// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The session layer: connect handshake, keepalive, request/response
//! correlation, and broadcast waiting.
//!
//! Sits between the transport (zone 100) and the client (zone 150). Owns a
//! background RX thread that reads frames, reassembles them, decodes the
//! protobuf message, and dispatches to waiters. Owns a keepalive thread that
//! pokes the device every 5 seconds.
//!
//! The session uses std threads + `Arc<HidDevice>` (no async runtime) so the
//! leaf crate stays embeddable. The session struct is `Send` but not `Clone`;
//! it owns the transport and the background threads.
//!
//! @see spec/140-session/spec.md [FR-1]
//! @see spec/140-session/design.md [DES-SESSION]

#![allow(clippy::missing_panics_doc, clippy::needless_pass_by_value)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::DeviceKind;
use crate::framing::{Frame, FrameReassembler, encode_message};
use crate::proto::cortex_message_type::Enum as MessageType;
use crate::proto::message_action::Enum as MessageAction;
use crate::proto::reset_comms_buffers_message::SessionId as RcbSessionId;
use crate::proto::{
    ConnectionMessage, CpuLoadMessage, KeepAliveMessage, ModelRepoMessage,
    ResetCommsBuffersMessage, VersionMessage,
};
use crate::transport::HID_REPORT_LEN;

/// The Cortex Control version string the host announces to the device.
/// The device gates state-push behaviour on receiving a valid CC version.
/// Captured on the wire against `CorOS` 4.0.1.
const CC_VERSION: &str = "4.0.1";

/// Default keepalive interval.
///
/// Measured from a capture of Cortex Control: 681 keepalives over 708 s, one
/// every 1.04 s. This was previously 5 s, on a comment asserting that was
/// what Cortex Control did - it is not, and the difference is not cosmetic.
/// Our subscribed sessions fell silent after roughly 40 s idle while CC's
/// never went quiet for more than 0.11 s over the same test.
const DEFAULT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(1);

/// After a request wait times out, a reply can still land in the race window.
/// Wait this long for the RX thread to finish delivering before declaring a
/// timeout.
const DELIVERY_GRACE: Duration = Duration::from_millis(500);

/// How often a blocked wait wakes to check whether the device has gone quiet.
///
/// A polling interval, not a timeout. Waking costs nothing and sends nothing;
/// it only bounds how late a verdict can be.
const SILENCE_POLL: Duration = Duration::from_millis(500);

/// How long the device may be completely silent before a wait gives up.
///
/// Set from the worst case, not the typical one. In steady state a kept-alive
/// session is never quiet - 0 s across a 90 s idle, and 0.11 s for Cortex
/// Control over the same test - but there is a lull straight after the
/// handshake, measured climbing to 4-5 s before the device begins streaming.
/// That lull is expected: the handshake ENDS by waiting for the device to go
/// quiet, so the count starts from a deliberately quiet moment.
///
/// A threshold of 5 s sat exactly on that boundary and refused a healthy
/// `version` seconds after connecting. Ten gives roughly double the observed
/// lull while still turning a dead device into a verdict in a third of the
/// 30 s request timeout.
///
/// All of this holds only while [`DEFAULT_KEEPALIVE_INTERVAL`] is frequent
/// enough. At 5 s the device stops pushing and healthy sessions fall silent
/// indefinitely, which is how an earlier version of this check was built on
/// sand, withdrawn, and rebuilt.
const SILENCE_LIMIT: Duration = Duration::from_secs(10);

/// Why a wait stopped waiting.
///
/// Distinguishing `Silent` from `TimedOut` is the point: "the device stopped
/// talking" and "my answer did not arrive" call for different reactions, and
/// collapsing them into one timeout is what made an unresponsive device look
/// like a hang.
enum WaitOutcome {
    /// The waiter was satisfied.
    Fired,
    /// The budget expired with the device still talking.
    TimedOut,
    /// Nothing arrived at all for this many seconds.
    Silent(u64),
}

/// Hard upper bound on reassembled body size. A legitimate message never
/// reaches this (the `ModelRepo` reply, ~47 KB, is the largest observed).
const MAX_MESSAGE_BODY: usize = 1 << 20; // 1 MiB

/// How long the RX thread blocks in a single `read_timeout` call.
///
/// This is deliberately SHORT and deliberately NOT
/// [`crate::transport::DEFAULT_READ_TIMEOUT`], which is the timeout for a
/// one-shot synchronous read. They are different concerns:
///
/// - `DEFAULT_READ_TIMEOUT` (2 s) is "how long to wait for an answer".
/// - This (200 ms) is "how long the RX thread holds the device mutex before
///   letting a writer in, and how quickly it notices `stop()`".
///
/// `hidapi::HidDevice` is `!Sync`, so reads and writes must share a mutex.
/// The RX thread holds it across its blocking read, which means every
/// `send()` waits for the current read to return. With a 2 s poll the
/// handshake's 25-message burst could serialise into tens of seconds of
/// contention; at 200 ms the worst case per send is bounded and small.
/// pyquadcortex uses the same 200 ms for the same reason.
const RX_POLL_TIMEOUT: Duration = Duration::from_millis(200);

/// How long the inbound stream must be silent before the connect handshake
/// considers the device's initial state dump finished.
const SETTLE_QUIET_PERIOD: Duration = Duration::from_millis(1500);

/// Ceiling on the adaptive settle, so a device that never goes quiet cannot
/// stall the handshake indefinitely.
const SETTLE_MAX: Duration = Duration::from_secs(30);

/// Message types the device pushes CONTINUOUSLY, regardless of whether it is
/// still sending state.
///
/// `GlobalTempo` is the tempo and metronome clock, not state. It has been
/// measured arriving roughly every 0.8 s in pairs, indefinitely. Counting it
/// as activity means the inbound stream is never silent, so a settle that
/// waits for silence can never finish and always runs to [`SETTLE_MAX`].
/// That was measured as a 30 s handshake on every command doing 9 ms of work,
/// which is why the exclusion exists.
///
/// It stops if the client stops earning it. Sessions that appeared to go
/// quiet for 17 s, 30 s and 80+ s were sending keepalives only every 5 s; at
/// the 1 s interval Cortex Control uses, the stream does not pause. The
/// exclusion here is still right - a settle waiting for silence must ignore a
/// clock - but the silence itself was ours. See roadmap PROT-008.6.4.
///
/// Anything listed here is ignored when deciding whether the device has
/// finished talking. It is still dispatched normally.
const HEARTBEAT_TYPES: &[MessageType] = &[MessageType::GlobalTempo, MessageType::IoMeter];

/// Whether inbound/outbound tracing is enabled, read once from the
/// `CORTEX_TRACE` environment variable.
///
/// The RX thread swallows errors by design (the benign write STALL, malformed
/// frames, unknown message types), so without this a failure is invisible.
/// Traces go to stderr so they can never corrupt a command's stdout.
fn trace_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("CORTEX_TRACE").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// Emit a trace line to stderr when `CORTEX_TRACE` is set.
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::session::trace_enabled() {
            eprintln!("cortex-trace: {}", format!($($arg)*));
        }
    };
}

/// The 22 state types the device requires a READ for before it starts pushing.
const SUBSCRIBE_TYPES: &[MessageType] = &[
    MessageType::ModuleStats,
    MessageType::License,
    MessageType::UndoRedo,
    MessageType::IoSettings,
    MessageType::GeneralSettings,
    MessageType::ShowGigView,
    MessageType::Mode,
    MessageType::GlobalEq,
    MessageType::MasterVolume,
    MessageType::File,
    MessageType::RecentsFavorites,
    MessageType::CompilerInhibitedModules,
    MessageType::RecallPreset,
    MessageType::NewModels,
    MessageType::PinnedModels,
    MessageType::DefaultParameters,
    MessageType::GlobalTempo,
    MessageType::SetlistPosition,
    MessageType::PresetDirty,
    MessageType::Scene,
    MessageType::BulkOperation,
    MessageType::Updater,
];

/// How much of the connect handshake to perform.
///
/// The full handshake subscribes to 22 state types, which is what makes the
/// device dump its entire state - on the unit measured, over 600 KB of folder
/// listings alone. That dump is necessary if the command is going to READ
/// anything, and pure overhead if it is only going to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectMode {
    /// Subscribe to device state and wait for the dump to finish.
    ///
    /// This is what a long-lived editor wants: the device then pushes state
    /// changes as they happen, without being asked.
    ///
    /// It is NOT needed to read: a targeted READ provokes its own reply.
    /// Measured on hardware - `cortex presets` took 50.6 s subscribed and
    /// 9.4 s minimal, returning the same listing, because subscribing makes
    /// the device dump every folder it knows and the command's own READ then
    /// makes it do so again.
    Subscribed,
    /// Announce the client and stop.
    ///
    /// Enough for the device to accept commands AND to answer targeted
    /// reads. Skips the 22 subscribe READs and the settle, so it neither
    /// provokes the state dump nor waits for one.
    ///
    /// The right default for a one-shot command. The `ModelRepo` READ is
    /// still sent, because that one IS load-bearing - without it the device
    /// answers nothing.
    Minimal,
}

/// A decoded inbound message: the type tag and the decoded protobuf body.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    /// The `CortexMessageType` tag from the trailer.
    pub message_type: MessageType,
    /// The raw protobuf body (trailer already stripped, gzip already
    /// decompressed). Callers decode this into the specific prost type.
    pub body: bytes::Bytes,
    /// The `request_id` if the message carries one. Extracted by a best-effort
    /// probe of the protobuf body (tag 2, uint64).
    pub request_id: Option<u64>,
}

/// A pending request waiter: an event + slot for the reply.
struct Waiter {
    expected_type: MessageType,
    event: Arc<(Mutex<bool>, Condvar)>,
    slot: Mutex<Option<InboundMessage>>,
}

/// A type waiter for unsolicited broadcasts.
struct TypeWaiter {
    expected_type: MessageType,
    predicate: Box<dyn Fn(&InboundMessage) -> bool + Send + Sync>,
    event: Arc<(Mutex<bool>, Condvar)>,
    slot: Mutex<Option<InboundMessage>>,
}

/// A collector: gathers EVERY matching message for a window rather than the
/// first one. Unlike a waiter, a collector does not consume messages - they
/// still reach any waiter or other collector.
struct Collector {
    expected_type: MessageType,
    predicate: Box<dyn Fn(&InboundMessage) -> bool + Send + Sync>,
    bucket: Mutex<Vec<InboundMessage>>,
}

/// Shared session state, accessed by the foreground (`request`/`await_broadcast`)
/// and the background RX thread.
struct Shared {
    /// Pending request waiters, keyed by `request_id`.
    pending: Mutex<HashMap<u64, Arc<Waiter>>>,
    /// Unsolicited-broadcast waiters.
    type_waiters: Mutex<Vec<Arc<TypeWaiter>>>,
    /// Active collectors, gathering every matching message for a window.
    collectors: Mutex<Vec<Arc<Collector>>>,
    /// Monotonic `request_id` counter.
    next_id: AtomicU64,
    /// Whether the session is running (signals the RX and keepalive threads).
    running: AtomicBool,
    /// Write serializer: ensures one logical message's reports are written as
    /// an atomic group. Separate from the state mutexes above.
    write_lock: Mutex<()>,
    /// How many threads are waiting to write to the device.
    ///
    /// The RX loop stands aside while this is non-zero. Without it a writer
    /// can starve for tens of seconds: the RX loop holds the device lock
    /// across a blocking read and reacquires it the instant it lets go, and
    /// Rust's `Mutex` is not fair, so "release, then yield" is a race the
    /// writer routinely loses. Measured on the wire: 46.8 s between the
    /// handshake's first and second messages, the bus silent throughout,
    /// and the device answering in 217 us once finally asked.
    writers_waiting: AtomicUsize,
    /// Milliseconds since `started` at which the last inbound message was
    /// dispatched. Lets `connect` tell "the device is still dumping state"
    /// from "the device has gone quiet", which a fixed sleep cannot.
    last_inbound_ms: AtomicU64,
    /// Session start, the origin for `last_inbound_ms`.
    started: Instant,
    /// Milliseconds at which ANY message last arrived, heartbeats included.
    ///
    /// Distinct from `last_inbound_ms`, which ignores heartbeats so the
    /// settle can tell "still dumping state" from "quiet". For liveness the
    /// heartbeat is precisely the signal we want.
    last_any_inbound_ms: AtomicU64,
    /// The most recent `CPULoad` push.
    ///
    /// Latest-wins rather than accumulated: this is a live gauge, and a
    /// reading from ten seconds ago is not worth keeping.
    cpu_load: Mutex<Option<CpuLoadMessage>>,
    /// The device's own `Version`, read during the handshake.
    ///
    /// Cached because the handshake asks for it anyway, and because a caller
    /// asking later would race the handshake's own announce: `Version` READ
    /// replies carry no `request_id`, so there is no way to tell one from the
    /// other on the wire.
    device_version: Mutex<Option<VersionMessage>>,
    /// The `ModelRepo` payload, captured from whichever reply arrives first.
    ///
    /// The handshake asks for this anyway - it is load-bearing - so a caller
    /// wanting the catalog would otherwise make the device build and send
    /// 46 KB a second time.
    model_repo: Mutex<Option<Vec<u8>>>,
}

/// The session owns a shared HID device handle, the background RX thread, the
/// keepalive thread, and the shared correlation state. It is `Send` but not
/// `Clone`; callers hold it by reference or behind an `Arc<Session>` at the
/// host layer.
pub struct Session {
    device: Arc<Mutex<hidapi::HidDevice>>,
    shared: Arc<Shared>,
    rx_handle: Mutex<Option<thread::JoinHandle<()>>>,
    keepalive_handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Session {
    /// Open a transport and start the background threads WITHOUT the connect
    /// handshake. The `version()` command uses this (a plain Version READ
    /// works without the handshake).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::DeviceNotFound`] if no matching device is on
    /// the bus, or [`crate::Error::Hid`] if `hidapi` fails to open the device.
    pub fn open(kind: DeviceKind) -> crate::Result<Self> {
        let transport = crate::Transport::open(kind)?;
        let device = Arc::new(Mutex::new(transport.into_device()));

        let shared = Arc::new(Shared {
            pending: Mutex::new(HashMap::new()),
            type_waiters: Mutex::new(Vec::new()),
            collectors: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            running: AtomicBool::new(true),
            write_lock: Mutex::new(()),
            writers_waiting: AtomicUsize::new(0),
            last_inbound_ms: AtomicU64::new(0),
            last_any_inbound_ms: AtomicU64::new(0),
            started: Instant::now(),
            cpu_load: Mutex::new(None),
            device_version: Mutex::new(None),
            model_repo: Mutex::new(None),
        });

        // RX thread: gets its own clone of the Arc<HidDevice>.
        let rx_device = Arc::clone(&device);
        let rx_shared = Arc::clone(&shared);
        let rx_handle = thread::Builder::new()
            .name("cortex-rx".into())
            .spawn(move || rx_loop(rx_device, rx_shared))
            .map_err(|e| crate::Error::Hid(format!("failed to spawn RX thread: {e}")))?;

        // Keepalive thread: also gets a clone for writing.
        let ka_device = Arc::clone(&device);
        let ka_shared = Arc::clone(&shared);
        let keepalive_handle = thread::Builder::new()
            .name("cortex-keepalive".into())
            .spawn(move || keepalive_loop(ka_device, ka_shared))
            .map_err(|e| crate::Error::Hid(format!("failed to spawn keepalive thread: {e}")))?;

        Ok(Self {
            device,
            shared,
            rx_handle: Mutex::new(Some(rx_handle)),
            keepalive_handle: Mutex::new(Some(keepalive_handle)),
        })
    }

    /// Draw a fresh `request_id` from the monotonic counter.
    #[must_use]
    pub fn next_request_id(&self) -> u64 {
        self.shared.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Send a fire-and-forget message (no correlation, no waiting).
    /// The writes are serialized under the write lock so a keepalive cannot
    /// interleave between a multi-report message's frames.
    ///
    /// # Errors
    ///
    /// Currently always returns `Ok` (write errors are swallowed per the
    /// benign STALL).
    pub fn send(&self, message_type: MessageType, payload: &[u8]) -> crate::Result<()> {
        let reports = encode_message(message_type as u16, payload);
        let _lock = self.shared.write_lock.lock().unwrap();
        // Announce the intent to write BEFORE contending for the device lock,
        // so the RX loop stops reacquiring it and this write gets through.
        let _intent = WriteIntent::new(&self.shared.writers_waiting);
        let device = self.device.lock().unwrap();
        for report in &reports {
            let _ = device.write(report);
        }
        Ok(())
    }

    /// Block until the waiter fires, the budget expires, or the device goes
    /// quiet.
    ///
    /// Polls in short slices rather than sleeping out the whole budget, so
    /// silence is noticed while it is happening rather than at the end.
    /// Purely observational: this sends NOTHING. Re-sending on this path was
    /// measured at roughly 3.5x worse - see roadmap PROT-008.5 before
    /// reaching for a retry here.
    ///
    /// Before the FIRST message of a session there is nothing to compare
    /// against, so the check stays disabled until the device has been heard
    /// from once. Otherwise every fresh session would look dead for its
    /// opening seconds, and `Version` is answered without a handshake, so a
    /// session opened only to ask for it may legitimately have heard nothing
    /// yet.
    fn wait_or_silence(&self, event: &(Mutex<bool>, Condvar), timeout: Duration) -> WaitOutcome {
        let deadline = Instant::now() + timeout;
        let (lock, cvar) = event;
        let mut fired = lock.lock().unwrap();
        while !*fired {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return WaitOutcome::TimedOut;
            }
            let slice = SILENCE_POLL.min(remaining);
            let (guard, _) = cvar.wait_timeout(fired, slice).unwrap();
            fired = guard;
            if *fired {
                break;
            }
            if self.has_heard_from_device() {
                let silent = self.seconds_since_last_message();
                if silent >= SILENCE_LIMIT.as_secs() {
                    return WaitOutcome::Silent(silent);
                }
            }
        }
        WaitOutcome::Fired
    }

    /// Send a message and block until the matching reply arrives. Correlates
    /// by message type first, `request_id` as consistency check.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if no reply arrives within
    /// `timeout`.
    pub fn request(
        &self,
        message_type: MessageType,
        payload: &[u8],
        request_id: u64,
        timeout: Duration,
    ) -> crate::Result<InboundMessage> {
        let event = Arc::new((Mutex::new(false), Condvar::new()));
        let waiter = Arc::new(Waiter {
            expected_type: message_type,
            event: event.clone(),
            slot: Mutex::new(None),
        });

        // Register BEFORE writing so a fast reply cannot race registration.
        self.shared
            .pending
            .lock()
            .unwrap()
            .insert(request_id, waiter.clone());

        // Send.
        self.send(message_type, payload)?;

        // Wait for the reply, the budget, or the device going quiet.
        match self.wait_or_silence(&event, timeout) {
            WaitOutcome::Fired => {}
            WaitOutcome::Silent(seconds) => {
                // Retire the waiter: nothing is coming, and leaving it
                // registered would strand it for the life of the session.
                self.shared.pending.lock().unwrap().remove(&request_id);
                return Err(crate::Error::DeviceSilent(seconds));
            }
            WaitOutcome::TimedOut => {
                // Check whether the RX thread already popped the entry.
                let delivered_by_rx = self
                    .shared
                    .pending
                    .lock()
                    .unwrap()
                    .remove(&request_id)
                    .is_none();
                if delivered_by_rx {
                    // The RX thread is committed to delivering; wait a grace.
                    let (lock, cvar) = &*event;
                    let result = lock.lock().unwrap();
                    if !*result {
                        let (result, _) = cvar.wait_timeout(result, DELIVERY_GRACE).unwrap();
                        if !*result {
                            return Err(crate::Error::ReadTimeout(timeout));
                        }
                    }
                } else {
                    return Err(crate::Error::ReadTimeout(timeout));
                }
            }
        }

        let slot = waiter.slot.lock().unwrap().take();
        slot.ok_or(crate::Error::ReadTimeout(timeout))
    }

    /// Register a type waiter, fire a trigger, and block for the next
    /// matching broadcast.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if no matching broadcast arrives.
    pub fn await_broadcast(
        &self,
        expected_type: MessageType,
        trigger: impl FnOnce(),
        timeout: Duration,
        predicate: impl Fn(&InboundMessage) -> bool + Send + Sync + 'static,
    ) -> crate::Result<InboundMessage> {
        let event = Arc::new((Mutex::new(false), Condvar::new()));
        let waiter = Arc::new(TypeWaiter {
            expected_type,
            predicate: Box::new(predicate),
            event: event.clone(),
            slot: Mutex::new(None),
        });

        // Register FIRST, then trigger.
        self.shared
            .type_waiters
            .lock()
            .unwrap()
            .push(waiter.clone());
        trigger();

        // Wait for a match, the budget, or the device going quiet.
        let outcome = self.wait_or_silence(&event, timeout);
        if !matches!(outcome, WaitOutcome::Fired) {
            // Retire the waiter first, so it cannot collect a late broadcast
            // that nobody is waiting for.
            {
                let mut waiters = self.shared.type_waiters.lock().unwrap();
                waiters.retain(|w| !Arc::ptr_eq(w, &waiter));
            }
            // A match can still have landed in the race window between the
            // wait giving up and the waiter being retired.
            if let Some(message) = waiter.slot.lock().unwrap().take() {
                return Ok(message);
            }
            return match outcome {
                WaitOutcome::Silent(seconds) => Err(crate::Error::DeviceSilent(seconds)),
                _ => Err(crate::Error::ReadTimeout(timeout)),
            };
        }

        let slot = waiter.slot.lock().unwrap().take();
        slot.ok_or(crate::Error::ReadTimeout(timeout))
    }

    /// Fire a trigger and gather EVERY matching message for `window`,
    /// returning them in arrival order.
    ///
    /// This is the counterpart to [`Session::await_broadcast`] for the case
    /// where one request provokes many pushes rather than one: a single
    /// `File` READ makes the device enumerate every folder it knows about
    /// (399 on the observed unit), arriving over ten to twenty seconds.
    ///
    /// Unlike a waiter, a collector does NOT consume messages - they still
    /// reach any waiter or other collector.
    ///
    /// Note this always blocks for the full `window`: there is no
    /// total-count field on the wire, so "have we got them all?" is
    /// unanswerable. Callers that can define completion in domain terms
    /// should poll instead (see the client's `wait_for_listing`).
    ///
    /// See spec/140-session/spec.md [FR-11].
    ///
    /// # Errors
    ///
    /// Currently always returns `Ok`. The error variant is kept for forward
    /// compatibility; an empty `Vec` means nothing matched, which is a
    /// domain-level outcome rather than a transport failure.
    pub fn collect(
        &self,
        expected_type: MessageType,
        trigger: impl FnOnce(),
        window: Duration,
        predicate: impl Fn(&InboundMessage) -> bool + Send + Sync + 'static,
    ) -> crate::Result<Vec<InboundMessage>> {
        let collector = Arc::new(Collector {
            expected_type,
            predicate: Box::new(predicate),
            bucket: Mutex::new(Vec::new()),
        });

        // Register FIRST, then trigger, so a fast push cannot arrive before
        // the collector is listening.
        self.shared
            .collectors
            .lock()
            .unwrap()
            .push(collector.clone());
        trigger();

        thread::sleep(window);

        // Deregister, then drain. Deregistering first means a push landing
        // during the drain cannot be silently dropped between the two steps.
        self.shared
            .collectors
            .lock()
            .unwrap()
            .retain(|c| !Arc::ptr_eq(c, &collector));

        let gathered = std::mem::take(&mut *collector.bucket.lock().unwrap());
        Ok(gathered)
    }

    /// Perform the full connect handshake. See spec/140-session/spec.md [FR-1].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if the `ResetCommsBuffers` reply
    /// does not arrive within `timeout`.
    pub fn connect(&self, timeout: Duration, settle: Duration) -> crate::Result<()> {
        self.connect_with_progress(ConnectMode::Subscribed, timeout, settle, |_| {})
    }

    /// The minimal handshake: enough to issue commands and targeted reads.
    ///
    /// # Errors
    ///
    /// As [`Session::connect`].
    pub fn connect_minimal(&self, timeout: Duration) -> crate::Result<()> {
        self.connect_with_progress(ConnectMode::Minimal, timeout, Duration::ZERO, |_| {})
    }

    /// Read the device's `Version` and keep it, best effort.
    ///
    /// Failure is not fatal: the identity is a convenience, and the
    /// handshake's job is to get the device pushing.
    fn read_device_version(&self, timeout: Duration) {
        // 2. Version READ, before announcing our own.
        //
        // Cortex Control does this, and the ordering earns its keep twice
        // over: it gets the device's identity into the session while nothing
        // else is competing for the reply, and it removes the race that made
        // a later `Version` READ unreliable - those replies carry no
        // `request_id`, so our own announce is indistinguishable from a
        // device reply to anyone waiting on the type alone.
        //
        // Costs about 0.7 s, measured against Cortex Control doing the same.
        trace!("handshake 2/7: Version READ");
        let read = encode_read_message(MessageType::Version);
        match self.await_broadcast(
            MessageType::Version,
            || {
                let _ = self.send(MessageType::Version, &read);
            },
            timeout,
            |m| !m.body.is_empty(),
        ) {
            Ok(reply) => match prost::Message::decode(reply.body.as_ref()) {
                Ok(v) => {
                    let v: VersionMessage = v;
                    trace!("handshake 2/7: device version received");
                    *self.shared.device_version.lock().unwrap() = Some(v);
                }
                Err(e) => trace!("handshake 2/7: version undecodable: {e}"),
            },
            // Not fatal. The identity is a convenience; the handshake's job
            // is to get the device pushing, and it can do that without this.
            Err(e) => trace!("handshake 2/7: no version reply ({e})"),
        }
    }

    /// Ask for the model catalog and wait for it to arrive.
    ///
    /// Waiting is the point - see the comment inside.
    fn fetch_catalog(&self, payload: &[u8], timeout: Duration) {
        // Wait for the catalog before sending anything else.
        //
        // Cortex Control paces its handshake this way: request, wait for the
        // reply, then the next request. Firing the whole handshake at once
        // instead makes the device serialise ~24 requests against a 46 KB
        // transfer, and the catalog stalls behind the pile-up - measured at
        // a single 4.4 s gap mid-transfer, where the reports either side of
        // it arrive 0.6 ms apart. Cortex Control sees no such gap and has the
        // whole catalog in 0.65 s.
        trace!("handshake 4/8: ModelRepo READ");
        match self.await_broadcast(
            MessageType::ModelRepo,
            || {
                let _ = self.send(MessageType::ModelRepo, payload);
            },
            timeout,
            // Only the real catalog, not an echo: it is tens of kilobytes.
            |m| m.body.len() > 1024,
        ) {
            Ok(reply) => trace!(
                "handshake 4/8: catalog received ({} bytes)",
                reply.body.len()
            ),
            // Not fatal on its own - the READ still gated the device's push
            // behaviour, which is the load-bearing part.
            Err(e) => trace!("handshake 4/8: catalog did not arrive ({e})"),
        }
    }

    /// The connect handshake, reporting each step to `progress`.
    ///
    /// The handshake is several seconds of silence otherwise, which reads as
    /// a hang. A caller with a terminal should surface these.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if the `ResetCommsBuffers` reply
    /// does not arrive within `timeout`.
    pub fn connect_with_progress(
        &self,
        mode: ConnectMode,
        timeout: Duration,
        settle: Duration,
        progress: impl Fn(&str),
    ) -> crate::Result<()> {
        // 1. ResetCommsBuffers with a fresh session_id.
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        let rid = self.next_request_id();
        let msg = ResetCommsBuffersMessage {
            session_id: Some(RcbSessionId::SessionId(session_id)),
            request_id: Some(crate::proto::reset_comms_buffers_message::RequestId::RequestId(rid)),
        };
        let payload = prost::Message::encode_to_vec(&msg);
        progress("resetting comms buffers");
        trace!("handshake 1/6: ResetCommsBuffers (rid={rid})");
        let _ = self.request(MessageType::ResetCommsBuffers, &payload, rid, timeout)?;
        trace!("handshake 1/6: reply received");

        progress("reading device version");
        self.read_device_version(timeout);

        // 3. Version UPDATE announcing cortex_control_version.
        let version_msg = VersionMessage {
            action: MessageAction::Update as i32,
            cortex_control_version: Some(
                crate::proto::version_message::CortexControlVersion::CortexControlVersion(
                    CC_VERSION.into(),
                ),
            ),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&version_msg);
        progress("announcing client version");
        trace!("handshake 2/6: Version UPDATE announce");
        self.send(MessageType::Version, &payload)?;

        // 3. ModelRepo READ.
        let repo_msg = ModelRepoMessage {
            action: MessageAction::Read as i32,
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&repo_msg);
        // LOAD-BEARING. This looks like a gratuitous 46 KB fetch during a
        // handshake and it is the obvious thing to "optimise" away, so:
        // removing it was measured on CorOS 4.0.1 (2026-08-02) and the
        // handshake dropped from ~35 s to 4.2 s, after which EVERY read
        // failed - active_scene, read_current_preset and list_presets all
        // timed out. The device gates its push behaviour on this request.
        //
        // The cost is the device's, not ours: the RX loop reassembles at
        // 1500+ reports/sec, while this one message trickles in at 82
        // reports/sec because the unit builds the catalog on demand.
        progress("requesting model catalog");
        self.fetch_catalog(&payload, timeout);

        // 4. Connection{connected: true}.
        let conn_msg = ConnectionMessage {
            request_id: None,
            connected: Some(crate::proto::connection_message::Connected::Connected(true)),
        };
        let payload = prost::Message::encode_to_vec(&conn_msg);
        progress("announcing connection");
        trace!("handshake 4/6: Connection connected=true");
        self.send(MessageType::Connection, &payload)?;

        // 5. 22 subscribe READs.
        if mode == ConnectMode::Minimal {
            // Stop here. The subscription is what provokes the state dump,
            // and a command-only session has nothing to do with it.
            trace!("handshake: minimal, skipping subscription and settle");
            return Ok(());
        }

        progress("subscribing to device state");
        trace!("handshake 5/6: {} subscribe READs", SUBSCRIBE_TYPES.len());
        for mt in SUBSCRIBE_TYPES {
            trace!("  subscribe {mt:?}");
            let payload = encode_read_message(*mt);
            self.send(*mt, &payload)?;
        }

        // CPU load is asked for differently, and the difference is load-
        // bearing. Every other subscribe is a plain READ; Cortex Control
        // sends this one with action CREATE and a `request_id`, which on the
        // wire is a single field 2 and no action at all - proto3 omits a
        // default, and CREATE is 0.
        //
        // Verified: a READ here is simply ignored. Our subscribe burst went
        // out with `CpuLoad` as a READ and the device never pushed a single
        // load message, while Cortex Control received roughly one a second
        // over the same kind of session. Reading it as "create a
        // subscription" rather than "read a value" also makes more sense of
        // a message whose reply is a continuous stream.
        let rid = self.next_request_id();
        let cpu = CpuLoadMessage {
            action: MessageAction::Create as i32,
            request_id: Some(crate::proto::cpu_load_message::RequestId::RequestId(rid)),
            ..Default::default()
        };
        trace!("handshake 5/7: CpuLoad CREATE (rid={rid})");
        let payload = prost::Message::encode_to_vec(&cpu);
        self.send(MessageType::CpuLoad, &payload)?;

        // 6. Settle until the device stops talking.
        //
        // A fixed sleep is the wrong shape here. The device services the
        // subscription burst LAZILY and in bulk - measured on CorOS 4.0.1,
        // a 46 KB ModelRepo and a 22 KB ModuleStats can begin arriving more
        // than ten seconds after the burst is sent. A caller that issues a
        // read as soon as a fixed settle expires finds its reply queued
        // behind tens of kilobytes of pushes and times out, which looks
        // exactly like a broken handshake.
        //
        // So wait for QUIET instead: the burst is done when nothing has
        // arrived for `SETTLE_QUIET_PERIOD`. `settle` becomes the floor (we
        // always wait at least that long) and `SETTLE_MAX` the ceiling, so a
        // chatty device cannot stall the handshake indefinitely.
        progress("settling");
        thread::sleep(settle);

        let deadline = Instant::now() + SETTLE_MAX;
        while Instant::now() < deadline {
            let idle_ms = self
                .shared
                .started
                .elapsed()
                .as_millis()
                .saturating_sub(u128::from(
                    self.shared.last_inbound_ms.load(Ordering::Relaxed),
                ));
            if idle_ms >= SETTLE_QUIET_PERIOD.as_millis() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        trace!("handshake 6/6: device quiet, handshake complete");

        Ok(())
    }

    /// Send `Connection{connected: false}` (best effort).
    pub fn disconnect(&self) {
        let msg = ConnectionMessage {
            request_id: None,
            connected: Some(crate::proto::connection_message::Connected::Connected(
                false,
            )),
        };
        let payload = prost::Message::encode_to_vec(&msg);
        let _ = self.send(MessageType::Connection, &payload);
    }

    /// Seconds since anything was received from the device.
    ///
    /// Counts ALL inbound traffic, heartbeats included - unlike the settle
    /// logic, which deliberately ignores them.
    ///
    /// A healthy subscribed session is never quiet for long. Measured
    /// against a keepalive of 1 s, this reads 0 throughout a 90 s idle; a
    /// capture of Cortex Control shows its longest inbound gap at 0.11 s.
    ///
    /// It was briefly believed that an idle session legitimately falls silent
    /// for 80+ s, and a fail-fast was withdrawn on that basis. That silence
    /// was our own doing - the keepalive interval was 5 s, and the device
    /// stops pushing when they are that sparse. With that fixed, silence is a
    /// usable signal again. See roadmap PROT-008.6.4.
    #[must_use]
    pub fn seconds_since_last_message(&self) -> u64 {
        let last = self.shared.last_any_inbound_ms.load(Ordering::Relaxed);
        let now = u64::try_from(self.shared.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        now.saturating_sub(last) / 1000
    }

    /// Whether anything has EVER been received on this session.
    ///
    /// [`Self::seconds_since_last_message`] measures from session start when
    /// nothing has arrived yet, which reads as a long silence on a session
    /// that is merely new. Anything deciding "is this device dead" must check
    /// here first, or it will condemn every session in its opening seconds.
    #[must_use]
    pub fn has_heard_from_device(&self) -> bool {
        self.shared.last_any_inbound_ms.load(Ordering::Relaxed) > 0
    }

    /// The most recent CPU load pushed by the device, if any has arrived.
    ///
    /// Only populated on a SUBSCRIBED session: the device pushes this because
    /// the handshake asked it to, and a `Minimal` session never does.
    #[must_use]
    pub fn cpu_load(&self) -> Option<CpuLoadMessage> {
        self.shared.cpu_load.lock().unwrap().clone()
    }

    /// The device's `Version` as read during the handshake, if it answered.
    ///
    /// `None` on a session that never handshook - `Version` is answered
    /// without one, so those sessions exist - or if the device did not reply.
    #[must_use]
    pub fn device_version(&self) -> Option<VersionMessage> {
        self.shared.device_version.lock().unwrap().clone()
    }

    /// The `ModelRepo` payload captured during this session, if one has
    /// arrived.
    ///
    /// The handshake requests it, so by the time a caller wants the catalog
    /// it has usually already been received. Returning it here avoids a
    /// second 46 KB transfer that the device builds from scratch.
    #[must_use]
    pub fn captured_model_repo(&self) -> Option<Vec<u8>> {
        self.shared.model_repo.lock().unwrap().clone()
    }

    /// Signal the background threads to exit and join them.
    pub fn stop(&self) {
        self.shared.running.store(false, Ordering::Relaxed);
        let mut rx = self.rx_handle.lock().unwrap();
        if let Some(handle) = rx.take() {
            let _ = handle.join();
        }
        let mut ka = self.keepalive_handle.lock().unwrap();
        if let Some(handle) = ka.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Announce the disconnect before tearing down.
        //
        // Without this the device is never told the client left; it simply
        // stops receiving keepalives, and meanwhile keeps pushing state to a
        // client that has gone. The NEXT session then contends with that
        // backlog, which shows up as reads timing out after an apparently
        // successful handshake.
        //
        // Best effort by nature: the send needs a live transport, and every
        // host write reports failure anyway thanks to the status-stage stall,
        // so swallowing the error is the normal path rather than a special
        // case.
        //
        // This covers normal drops and error returns. It does NOT cover
        // SIGINT or SIGTERM, which terminate without running destructors -
        // see the signal handler in the CLI.
        self.disconnect();
        self.stop();
    }
}

/// Encode a bare `{action: READ}` message. Most message types have an `action`
/// field at tag 1 (varint); this produces the minimal protobuf for READ = 3.
fn encode_read_message(_mt: MessageType) -> Vec<u8> {
    // action is tag 1, wire_type varint (0). key = (1 << 3) | 0 = 0x08.
    // value = 3 (READ).
    vec![0x08, 0x03]
}

/// Best-effort extraction of `request_id` from a protobuf body. The
/// `request_id` field is tag 2, uint64 (wire type 0 = varint). This scans the
/// raw bytes for that tag without fully decoding the message.
fn extract_request_id(body: &[u8]) -> Option<u64> {
    let mut pos = 0;
    while pos < body.len() {
        let (tag, consumed) = decode_varint(&body[pos..])?;
        pos += consumed;
        let field_number = tag >> 3;
        let wire_type = tag & 0x07;
        if field_number == 2 && wire_type == 0 {
            let (value, c) = decode_varint(&body[pos..])?;
            let _ = c;
            return Some(value);
        }
        // Skip this field based on wire type.
        match wire_type {
            0 => {
                let (_, c) = decode_varint(&body[pos..])?;
                pos += c;
            }
            1 => pos += 8,
            2 => {
                let (len, c) = decode_varint(&body[pos..])?;
                #[allow(clippy::cast_possible_truncation)]
                let len_usize = len as usize;
                pos += c + len_usize;
            }
            5 => pos += 4,
            _ => return None,
        }
    }
    None
}

/// Decode a protobuf varint from the start of a byte slice. Returns
/// (value, bytes consumed).
fn decode_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        result |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some((result, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

/// How long the RX loop waits between checks while standing aside for a
/// writer. Short, because it delays every write by up to this much.
const WRITER_BACKOFF: Duration = Duration::from_millis(1);

/// Marks a thread as waiting to write, so the RX loop stands aside.
///
/// RAII rather than a bare increment and decrement: a panic between
/// announcing the intent and finishing the write would otherwise leave the
/// count above zero forever, and the RX loop would keep standing aside for a
/// writer that no longer exists - turning a transient fault into a session
/// that receives nothing at all.
struct WriteIntent<'a>(&'a AtomicUsize);

impl<'a> WriteIntent<'a> {
    fn new(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::AcqRel);
        Self(counter)
    }
}

impl Drop for WriteIntent<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The background RX loop: read frames, reassemble, decode, dispatch.
fn rx_loop(device: Arc<Mutex<hidapi::HidDevice>>, shared: Arc<Shared>) {
    let mut reassembler = FrameReassembler::new();
    let mut report_count: usize = 0;
    let mut partial_started = Instant::now();

    while shared.running.load(Ordering::Relaxed) {
        let mut buf = vec![0u8; HID_REPORT_LEN];
        let timeout_ms =
            i32::try_from(RX_POLL_TIMEOUT.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX);
        // Stand aside for any thread waiting to write, BEFORE taking the
        // device lock. Yielding after releasing it is not enough: the writer
        // and this loop then race for an unfair mutex, and with a backlog to
        // read the loop wins almost every time.
        while shared.writers_waiting.load(Ordering::Acquire) > 0
            && shared.running.load(Ordering::Relaxed)
        {
            thread::sleep(WRITER_BACKOFF);
        }

        let n = {
            let device = device.lock().unwrap();
            if let Ok(n) = device.read_timeout(&mut buf, timeout_ms) {
                n
            } else {
                reassembler.reset();
                report_count = 0;
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        };
        // Hand over promptly to a writer that arrived while the lock was
        // held. This is only a latency tweak; the guarantee comes from the
        // `writers_waiting` gate at the top of the loop.
        //
        // A bare yield WAS the whole mitigation, and it does not work. It
        // leaves writer and reader racing for an unfair mutex, and with a
        // backlog to read the reader wins: 46.8 s of bus silence between the
        // handshake's first and second messages, on a device answering in
        // 217 us. Keep the gate; this line alone is not a fix.
        thread::yield_now();

        if n == 0 {
            // Timeout, not an error; loop and check running.
            continue;
        }
        buf.truncate(n);
        let report = buf;

        let Ok(frame) = Frame::parse(&report) else {
            reassembler.reset();
            report_count = 0;
            continue;
        };

        // A FIRST frame always begins a new message; drop any stale partial.
        if frame.flags.is_first() && report_count > 0 {
            reassembler.reset();
            report_count = 0;
        }

        match reassembler.feed(&frame) {
            Ok(Some(body)) => {
                if report_count > 0 {
                    // Throughput matters here: a slow message is the device
                    // trickling, not us. Reporting both figures lets the two
                    // be told apart without a calculator.
                    trace!(
                        "reassembled {} reports in {} ms",
                        report_count + 1,
                        partial_started.elapsed().as_millis()
                    );
                }
                report_count = 0;
                if let Err(e) = handle_message(&body, &shared) {
                    // Routine, not exceptional: the device pushes types we do
                    // not decode (License and CloudLogin carry non-protobuf
                    // bodies). Tracing, not stderr noise.
                    trace!("undecodable inbound message: {e}");
                }
            }
            Ok(None) => {
                if report_count == 0 {
                    partial_started = Instant::now();
                }
                report_count += 1;
                if report_count > MAX_MESSAGE_BODY / crate::framing::CHUNK_SIZE {
                    reassembler.reset();
                    report_count = 0;
                }
            }
            Err(_) => {
                reassembler.reset();
                report_count = 0;
            }
        }
    }
}

/// Decode a reassembled body, decompress if needed, and dispatch to waiters.
fn handle_message(body: &[u8], shared: &Shared) -> crate::Result<()> {
    // Trailer first, then gzip. Shared with the offline decoder so the two
    // cannot drift - see `Message::decode`.
    let (msg, _gzipped) = crate::message::Message::decode(body)?;

    let mt = MessageType::try_from(i32::from(msg.message_type)).unwrap_or(MessageType::Undefined);
    let request_id = extract_request_id(&msg.body);

    let inbound = InboundMessage {
        message_type: mt,
        body: msg.body,
        request_id,
    };

    dispatch(&inbound, shared);
    Ok(())
}

/// Route an inbound message to any matching waiters, type waiters, and collectors.
fn dispatch(msg: &InboundMessage, shared: &Shared) {
    // Keep the first ModelRepo payload we see. The handshake asks for one
    // anyway, so a later catalog request can be answered without making the
    // device build and send 46 KB again.
    if msg.message_type == MessageType::ModelRepo {
        if let Some(payload) = extract_model_repo_payload(&msg.body) {
            let mut slot = shared.model_repo.lock().unwrap();
            if slot.is_none() {
                trace!("captured ModelRepo payload ({} bytes)", payload.len());
                *slot = Some(payload);
            }
        }
    }
    // Keep the latest CPU load, so a caller can read it without waiting for
    // the next push - which may be up to a second away.
    if msg.message_type == MessageType::CpuLoad {
        if let Ok(load) = prost::Message::decode(msg.body.as_ref()) {
            let load: CpuLoadMessage = load;
            *shared.cpu_load.lock().unwrap() = Some(load);
        }
    }
    // Liveness counts everything: a heartbeat is exactly what tells us the
    // link is still up.
    shared.last_any_inbound_ms.store(
        u64::try_from(shared.started.elapsed().as_millis()).unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
    // The settle, by contrast, must ignore heartbeats or it can never
    // observe the device going quiet.
    if !HEARTBEAT_TYPES.contains(&msg.message_type) {
        shared.last_inbound_ms.store(
            u64::try_from(shared.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }
    trace!(
        "rx {:?} ({} bytes) request_id={:?}",
        msg.message_type,
        msg.body.len(),
        msg.request_id
    );
    // Collectors observe rather than consume, so they are fed FIRST - before
    // any of the early returns below. A message claimed by a waiter still
    // reaches every matching collector.
    {
        let collectors = shared.collectors.lock().unwrap();
        for collector in collectors.iter() {
            if collector.expected_type == msg.message_type && (collector.predicate)(msg) {
                collector.bucket.lock().unwrap().push(msg.clone());
            }
        }
    }

    // Pending request waiters: match by type, then by request_id if both have one.
    let matched = {
        let mut pending = shared.pending.lock().unwrap();
        let mut matched = None;
        // First try: if the message has a request_id, match it to a pending waiter.
        if let Some(rid) = msg.request_id {
            if let Some(waiter) = pending.get(&rid) {
                if waiter.expected_type == msg.message_type {
                    matched = Some(pending.remove(&rid).unwrap());
                }
            }
        }
        // Second try: if no request_id on the message, match the LOWEST-id
        // same-type waiter. This handles READ replies, which carry no
        // request_id echo (spec/140-session/spec.md [FR-8]).
        //
        // The minimum matters: `pending` is a HashMap, whose iteration order
        // is arbitrary, so taking the first match found would non-
        // deterministically satisfy the wrong waiter when two same-type
        // requests are in flight. Lowest id = oldest request = the one that
        // has been waiting longest.
        if matched.is_none() && msg.request_id.is_none() {
            let best_key = pending
                .iter()
                .filter(|(_, waiter)| waiter.expected_type == msg.message_type)
                .map(|(&key, _)| key)
                .min();
            if let Some(key) = best_key {
                matched = Some(pending.remove(&key).unwrap());
            }
        }
        matched
    };

    if let Some(waiter) = matched {
        *waiter.slot.lock().unwrap() = Some(msg.clone());
        let (lock, cvar) = &*waiter.event;
        *lock.lock().unwrap() = true;
        cvar.notify_one();
        return;
    }

    // Type waiters (unsolicited broadcasts).
    let mut type_waiters = shared.type_waiters.lock().unwrap();
    for i in 0..type_waiters.len() {
        if type_waiters[i].expected_type == msg.message_type && (type_waiters[i].predicate)(msg) {
            let waiter = type_waiters.remove(i);
            *waiter.slot.lock().unwrap() = Some(msg.clone());
            let (lock, cvar) = &*waiter.event;
            *lock.lock().unwrap() = true;
            cvar.notify_one();
            return;
        }
    }
}

/// The background keepalive loop: send KeepAlive{UPDATE} every
/// [`DEFAULT_KEEPALIVE_INTERVAL`].
fn keepalive_loop(device: Arc<Mutex<hidapi::HidDevice>>, shared: Arc<Shared>) {
    while shared.running.load(Ordering::Relaxed) {
        // Sleep in short slices rather than one 5 s block. `stop()` joins
        // this thread, so a plain sleep makes every teardown wait up to a
        // full interval - measured at 1.4 s on average and up to 5 s, paid
        // by every command.
        let slept = {
            let mut elapsed = Duration::ZERO;
            while elapsed < DEFAULT_KEEPALIVE_INTERVAL {
                if !shared.running.load(Ordering::Relaxed) {
                    return;
                }
                let remaining = DEFAULT_KEEPALIVE_INTERVAL.saturating_sub(elapsed);
                let slice = Duration::from_millis(50).min(remaining);
                thread::sleep(slice);
                elapsed += slice;
            }
            elapsed
        };
        let _ = slept;
        if !shared.running.load(Ordering::Relaxed) {
            return;
        }
        let msg = KeepAliveMessage {
            action: MessageAction::Update as i32,
            request_id: None,
            is_online: true,
            timeout: None,
        };
        let payload = prost::Message::encode_to_vec(&msg);
        let reports = encode_message(MessageType::KeepAlive as u16, &payload);
        let _wlock = shared.write_lock.lock().unwrap();
        // Announce the write like any other, so the RX loop stands aside.
        //
        // This loop takes the locks itself rather than going through
        // `Session::send`, and so was left out when writer starvation was
        // fixed everywhere else. The cost was quiet but real: keepalives
        // configured at 1 s went out every ~2.3 s, because each one queued
        // behind a reader that reacquires the device lock the instant it lets
        // go. Keepalive spacing is not cosmetic - the device stops pushing
        // state when they are too sparse.
        let _intent = WriteIntent::new(&shared.writers_waiting);
        let device = device.lock().unwrap();
        for report in &reports {
            let _ = device.write(report);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_decodes_correctly() {
        // Tag 2, varint: key = (2 << 3) | 0 = 0x10, value = 42.
        let bytes = [0x10, 0x2A];
        let rid = extract_request_id(&bytes);
        assert_eq!(rid, Some(42));
    }

    #[test]
    fn varint_skips_non_matching_fields() {
        // Tag 1, varint (action): key = 0x08, value = 3 (READ).
        // Tag 2, varint (request_id): key = 0x10, value = 99.
        let bytes = [0x08, 0x03, 0x10, 0x63];
        let rid = extract_request_id(&bytes);
        assert_eq!(rid, Some(99));
    }

    #[test]
    fn varint_handles_no_request_id() {
        // Only tag 1 (action), no tag 2.
        let bytes = [0x08, 0x03];
        let rid = extract_request_id(&bytes);
        assert_eq!(rid, None);
    }

    #[test]
    fn varint_skips_length_delimited_fields() {
        // Tag 1, varint: 0x08, 0x03
        // Tag 3, length-delimited: key = (3 << 3) | 2 = 0x1A, len = 5, "hello"
        // Tag 2, varint: key = 0x10, value = 7
        let bytes = [
            0x08, 0x03, // action = READ
            0x1A, 0x05, b'h', b'e', b'l', b'l', b'o', // field 3 = "hello"
            0x10, 0x07, // request_id = 7
        ];
        let rid = extract_request_id(&bytes);
        assert_eq!(rid, Some(7));
    }

    #[test]
    fn encode_read_message_produces_action_read() {
        let payload = encode_read_message(MessageType::Version);
        assert_eq!(payload, vec![0x08, 0x03]);
    }
}

#[cfg(test)]
mod correlation_tests {
    //! Tests for the routing rules in [`dispatch`].
    //!
    //! These are the highest-risk logic in the crate: every failure mode here
    //! is SILENT. A reply delivered to the wrong waiter does not error, it
    //! returns the wrong data, and the caller has no way to tell. Hardware
    //! runs exercise one happy path per method and would not catch any of it.
    //!
    //! `dispatch` is a free function over `(&InboundMessage, &Shared)`, so
    //! this needs no device and no fake transport - the correlation rules can
    //! be driven directly.
    //!
    //! @see spec/140-session/spec.md [FR-7] [FR-8] [FR-10] [FR-11]

    use super::*;

    fn shared() -> Shared {
        Shared {
            pending: Mutex::new(HashMap::new()),
            type_waiters: Mutex::new(Vec::new()),
            collectors: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            running: AtomicBool::new(true),
            write_lock: Mutex::new(()),
            writers_waiting: AtomicUsize::new(0),
            last_inbound_ms: AtomicU64::new(0),
            last_any_inbound_ms: AtomicU64::new(0),
            started: Instant::now(),
            cpu_load: Mutex::new(None),
            device_version: Mutex::new(None),
            model_repo: Mutex::new(None),
        }
    }

    fn message(message_type: MessageType, request_id: Option<u64>) -> InboundMessage {
        InboundMessage {
            message_type,
            body: bytes::Bytes::from_static(b"\x08\x03"),
            request_id,
        }
    }

    /// Register a request waiter and hand back the handle to inspect it.
    fn expect(shared: &Shared, id: u64, message_type: MessageType) -> Arc<Waiter> {
        let waiter = Arc::new(Waiter {
            expected_type: message_type,
            event: Arc::new((Mutex::new(false), Condvar::new())),
            slot: Mutex::new(None),
        });
        shared.pending.lock().unwrap().insert(id, waiter.clone());
        waiter
    }

    fn delivered(waiter: &Waiter) -> Option<MessageType> {
        waiter.slot.lock().unwrap().as_ref().map(|m| m.message_type)
    }

    // -- request/reply correlation ----------------------------------------

    #[test]
    fn a_read_reply_with_no_id_matches_on_type_alone() {
        // READ replies carry no request_id echo, so type is all there is.
        let s = shared();
        let w = expect(&s, 1, MessageType::Version);
        dispatch(&message(MessageType::Version, None), &s);
        assert_eq!(delivered(&w), Some(MessageType::Version));
        assert!(
            s.pending.lock().unwrap().is_empty(),
            "waiter was not consumed"
        );
    }

    #[test]
    fn a_reply_with_an_id_goes_to_that_waiter() {
        let s = shared();
        let first = expect(&s, 1, MessageType::SetlistPosition);
        let second = expect(&s, 2, MessageType::SetlistPosition);
        dispatch(&message(MessageType::SetlistPosition, Some(2)), &s);
        assert_eq!(delivered(&second), Some(MessageType::SetlistPosition));
        assert_eq!(delivered(&first), None, "the wrong waiter was satisfied");
    }

    #[test]
    fn id_less_replies_drain_waiters_oldest_first() {
        // The regression this guards: `pending` is a HashMap, so taking the
        // first same-type match FOUND picks arbitrarily. Lowest id means
        // oldest request, which is the one that has waited longest.
        //
        // Asserting the whole ORDER rather than one delivery is deliberate.
        // With two waiters the buggy implementation still picks correctly
        // about half the time - Rust randomises HashMap iteration per
        // process - so a single-delivery assertion fails only intermittently
        // and would wave the regression through most runs. Measured: 4
        // failures in 12 runs. Requiring six replies to land in ascending id
        // order leaves the buggy version roughly a 1-in-720 chance of
        // passing, which is a guard rather than a coin toss.
        let s = shared();
        let ids = [11_u64, 4, 27, 2, 19, 8];
        let waiters: Vec<(u64, Arc<Waiter>)> = ids
            .iter()
            .map(|&id| (id, expect(&s, id, MessageType::Version)))
            .collect();

        let mut order = Vec::new();
        for _ in 0..ids.len() {
            dispatch(&message(MessageType::Version, None), &s);
            // Whichever waiter just filled is the one dispatch chose.
            let just_filled = waiters
                .iter()
                .find(|(id, w)| w.slot.lock().unwrap().is_some() && !order.contains(id))
                .map(|(id, _)| *id)
                .expect("a waiter should have been satisfied");
            order.push(just_filled);
        }

        let mut ascending = ids;
        ascending.sort_unstable();
        assert_eq!(
            order,
            ascending.to_vec(),
            "id-less replies must drain oldest-first; got {order:?}"
        );
    }

    #[test]
    fn a_cascade_message_of_another_type_is_not_a_reply() {
        // A state-changing request provokes a cascade of OTHER types that all
        // echo its request_id. Matching on the id alone would deliver the
        // first of those as if it were the reply.
        let s = shared();
        let w = expect(&s, 1, MessageType::SetlistPosition);
        dispatch(&message(MessageType::UndoRedo, Some(1)), &s);
        assert_eq!(
            delivered(&w),
            None,
            "a cascade message was taken as the reply"
        );
        assert_eq!(
            s.pending.lock().unwrap().len(),
            1,
            "the waiter was consumed"
        );
    }

    #[test]
    fn an_unmatched_message_is_dropped_without_disturbing_waiters() {
        let s = shared();
        let w = expect(&s, 1, MessageType::Scene);
        dispatch(&message(MessageType::CpuLoad, None), &s);
        assert_eq!(delivered(&w), None);
        assert_eq!(s.pending.lock().unwrap().len(), 1);
    }

    // -- broadcast waiters -------------------------------------------------

    fn watch(
        shared: &Shared,
        message_type: MessageType,
        predicate: impl Fn(&InboundMessage) -> bool + Send + Sync + 'static,
    ) -> Arc<TypeWaiter> {
        let waiter = Arc::new(TypeWaiter {
            expected_type: message_type,
            predicate: Box::new(predicate),
            event: Arc::new((Mutex::new(false), Condvar::new())),
            slot: Mutex::new(None),
        });
        shared.type_waiters.lock().unwrap().push(waiter.clone());
        waiter
    }

    #[test]
    fn a_broadcast_waiter_skips_the_stale_seed_push() {
        // This is exactly how read_preset works, and the trap it avoids: the
        // handshake's subscription produces an unsolicited RecallPreset push
        // carrying NO request_id, while the push caused by our own recall
        // echoes it. Accepting the first RecallPreset to arrive returns the
        // preset from the PREVIOUS recall - correct-looking, wrong data.
        let s = shared();
        let w = watch(&s, MessageType::RecallPreset, |m| m.request_id == Some(9));

        dispatch(&message(MessageType::RecallPreset, None), &s);
        assert_eq!(delivered_type(&w), None, "the seed push was accepted");
        assert_eq!(
            s.type_waiters.lock().unwrap().len(),
            1,
            "a rejected candidate must leave the waiter registered"
        );

        dispatch(&message(MessageType::RecallPreset, Some(9)), &s);
        assert_eq!(delivered_type(&w), Some(MessageType::RecallPreset));
        assert!(s.type_waiters.lock().unwrap().is_empty());
    }

    fn delivered_type(waiter: &TypeWaiter) -> Option<MessageType> {
        waiter.slot.lock().unwrap().as_ref().map(|m| m.message_type)
    }

    #[test]
    fn a_request_waiter_wins_over_a_broadcast_waiter() {
        // Both could match. A correlated reply must satisfy the request that
        // asked for it, not a bystander watching the same type.
        let s = shared();
        let requester = expect(&s, 1, MessageType::Scene);
        let watcher = watch(&s, MessageType::Scene, |_| true);
        dispatch(&message(MessageType::Scene, Some(1)), &s);
        assert_eq!(delivered(&requester), Some(MessageType::Scene));
        assert_eq!(delivered_type(&watcher), None);
    }

    // -- collectors --------------------------------------------------------

    fn gather(shared: &Shared, message_type: MessageType) -> Arc<Collector> {
        let collector = Arc::new(Collector {
            expected_type: message_type,
            predicate: Box::new(|_| true),
            bucket: Mutex::new(Vec::new()),
        });
        shared.collectors.lock().unwrap().push(collector.clone());
        collector
    }

    #[test]
    fn a_collector_observes_without_consuming() {
        // list_folders collects while other callers may be waiting on the
        // same type. A collector that consumed would starve them.
        let s = shared();
        let c = gather(&s, MessageType::File);
        let w = expect(&s, 1, MessageType::File);
        dispatch(&message(MessageType::File, None), &s);
        assert_eq!(c.bucket.lock().unwrap().len(), 1, "collector missed it");
        assert_eq!(delivered(&w), Some(MessageType::File), "waiter was starved");
    }

    #[test]
    fn a_collector_gathers_every_matching_message() {
        // One File READ provokes hundreds of folder pushes; a single-shot
        // waiter would see only the first.
        let s = shared();
        let c = gather(&s, MessageType::File);
        for _ in 0..5 {
            dispatch(&message(MessageType::File, None), &s);
        }
        assert_eq!(c.bucket.lock().unwrap().len(), 5);
    }

    #[test]
    fn collectors_do_not_gather_other_types() {
        let s = shared();
        let c = gather(&s, MessageType::File);
        dispatch(&message(MessageType::Scene, None), &s);
        assert!(c.bucket.lock().unwrap().is_empty());
    }

    #[test]
    fn several_collectors_all_see_the_same_message() {
        let s = shared();
        let a = gather(&s, MessageType::File);
        let b = gather(&s, MessageType::File);
        dispatch(&message(MessageType::File, None), &s);
        assert_eq!(a.bucket.lock().unwrap().len(), 1);
        assert_eq!(b.bucket.lock().unwrap().len(), 1);
    }

    // -- liveness ----------------------------------------------------------

    #[test]
    fn dispatch_stamps_the_last_inbound_time() {
        // The adaptive settle in connect() depends on this: without the
        // stamp it can never observe the device going quiet and would always
        // wait the full SETTLE_MAX.
        let s = shared();
        assert_eq!(s.last_inbound_ms.load(Ordering::Relaxed), 0);
        thread::sleep(Duration::from_millis(5));
        dispatch(&message(MessageType::Scene, None), &s);
        assert!(
            s.last_inbound_ms.load(Ordering::Relaxed) > 0,
            "connect() would never see the device go quiet"
        );
    }
}

/// Pull the payload out of a `ModelRepo` message body, if it carries one.
///
/// The device also sends payload-less `ModelRepo` messages, which are not
/// the catalog.
fn extract_model_repo_payload(body: &[u8]) -> Option<Vec<u8>> {
    use crate::proto::{ModelRepoMessage, model_repo_message as mrm};
    let decoded: ModelRepoMessage = prost::Message::decode(body).ok()?;
    let mrm::ModelRepoPayload::ModelRepoPayload(payload) = decoded.model_repo_payload?;
    if payload.is_empty() {
        return None;
    }
    Some(payload)
}
