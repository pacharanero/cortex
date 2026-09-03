// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The session layer: connect handshake, keepalive, request/response
//! correlation, and broadcast waiting.
//!
//! Sits between the transport (zone 100) and the client (zone 150). Owns a
//! background RX thread that reads frames, reassembles them, decodes the
//! protobuf message, and dispatches to waiters. Owns a keepalive thread that
//! pokes the device every second.
//!
//! The session uses std threads + `Arc<HidDevice>` (no async runtime) so the
//! leaf crate stays embeddable. The session struct is `Send` but not `Clone`;
//! it owns the transport and the background threads.
//!
//! @see spec/140-session/spec.md [FR-1]
//! @see spec/140-session/design.md [DES-SES-OVR]

#![allow(clippy::missing_panics_doc, clippy::needless_pass_by_value)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(feature = "hid")]
use crate::DeviceKind;
use crate::framing::HID_REPORT_LEN;
use crate::framing::{Frame, FrameReassembler, encode_message};
use crate::proto::cortex_message_type::Enum as MessageType;
use crate::proto::message_action::Enum as MessageAction;
use crate::proto::reset_comms_buffers_message::SessionId as RcbSessionId;
use crate::proto::{
    ConnectionMessage, CpuLoadMessage, KeepAliveMessage, ModelRepoMessage,
    ResetCommsBuffersMessage, VersionMessage,
};
use crate::state::DeviceStateCache;

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
    /// The owning session was closed while waiting.
    Closed,
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
/// - This (200 ms) is "how quickly the RX loop notices `stop()` and rechecks
///   whether a writer is waiting".
///
/// `hidapi::HidDevice` is `!Sync`, so reads and writes must share a mutex.
/// The RX thread holds it across its blocking read, so a writer may wait for
/// the current poll. The timeout alone does not bound writer latency: an
/// unfair mutex let the reader reacquire repeatedly and starve writes for
/// tens of seconds. The `writers_waiting` gate gives a declared writer
/// priority; this short poll only bounds how soon the reader observes it.
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
pub(crate) fn trace_enabled() -> bool {
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
    /// Whether anything has ever been received.
    ///
    /// Explicit rather than inferred from `last_any_inbound_ms != 0`, because
    /// that field holds milliseconds since session start and 0 is a real
    /// timestamp for the first millisecond - so "arrived immediately" and
    /// "never arrived" were indistinguishable. Hardware hid it: the handshake
    /// takes seconds, so nothing ever landed inside that window.
    heard_anything: AtomicBool,
    /// Milliseconds at which ANY message last arrived, heartbeats included.
    ///
    /// Distinct from `last_inbound_ms`, which ignores heartbeats so the
    /// settle can tell "still dumping state" from "quiet". For liveness the
    /// heartbeat is precisely the signal we want.
    last_any_inbound_ms: AtomicU64,
    /// Non-consuming reducer fed before request/broadcast waiters.
    state: DeviceStateCache,
    /// Physical-session generation accepted by `state`.
    generation: u64,
}

/// The session owns a shared HID device handle, the background RX thread, the
/// keepalive thread, and the shared correlation state. It is `Send` but not
/// `Clone`; callers hold it by reference or behind an `Arc<Session>` at the
/// host layer.
pub struct Session {
    device: Arc<Mutex<Option<Box<dyn crate::link::HidLink>>>>,
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
    #[cfg(feature = "hid")]
    pub fn open(kind: DeviceKind) -> crate::Result<Self> {
        Self::open_with_state(kind, DeviceStateCache::new())
    }

    /// Open a transport while retaining a caller-owned cache handle.
    ///
    /// This is the reconnect seam: a host can invalidate one stable cache,
    /// replace the physical session, and keep existing observers attached.
    ///
    /// # Errors
    ///
    /// As [`Self::open`].
    #[cfg(feature = "hid")]
    pub fn open_with_state(kind: DeviceKind, state: DeviceStateCache) -> crate::Result<Self> {
        if kind != DeviceKind::QuadCortex {
            return Err(crate::Error::UnsupportedDeviceOperation {
                device: kind,
                operation: "Quad Cortex session",
            });
        }
        let transport = crate::Transport::open(kind)?;
        Self::over_with_state(transport.into_device(), state)
    }

    /// Start a session over any [`HidLink`](crate::link::HidLink).
    ///
    /// The substitutable entry point: `open` is this with a real device
    /// attached. Exists so the RX loop, the handshake and the writer gate can
    /// be tested at all - none of them were reachable without hardware, and
    /// they are where this project's costly bugs have lived.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Hid`] if a background thread cannot be spawned.
    pub fn over(device: impl crate::link::HidLink + 'static) -> crate::Result<Self> {
        Self::over_with_state(device, DeviceStateCache::new())
    }

    /// Start a session over a substitutable link and a retained cache handle.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Session`] if a background thread cannot be
    /// spawned.
    pub fn over_with_state(
        device: impl crate::link::HidLink + 'static,
        state: DeviceStateCache,
    ) -> crate::Result<Self> {
        let device: Arc<Mutex<Option<Box<dyn crate::link::HidLink>>>> =
            Arc::new(Mutex::new(Some(Box::new(device))));
        let generation = state.begin_generation();
        let shared = Arc::new(Shared {
            pending: Mutex::new(HashMap::new()),
            type_waiters: Mutex::new(Vec::new()),
            collectors: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            running: AtomicBool::new(true),
            write_lock: Mutex::new(()),
            writers_waiting: AtomicUsize::new(0),
            last_inbound_ms: AtomicU64::new(0),
            heard_anything: AtomicBool::new(false),
            last_any_inbound_ms: AtomicU64::new(0),
            started: Instant::now(),
            state,
            generation,
        });

        // RX thread: gets its own clone of the Arc<HidDevice>.
        let rx_device = Arc::clone(&device);
        let rx_shared = Arc::clone(&shared);
        let rx_handle = thread::Builder::new()
            .name("cortex-rx".into())
            .spawn(move || rx_loop(rx_device, rx_shared))
            .map_err(|e| crate::Error::Session(format!("failed to spawn RX thread: {e}")))?;

        // Keepalive thread: also gets a clone for writing.
        let ka_device = Arc::clone(&device);
        let ka_shared = Arc::clone(&shared);
        let keepalive_handle = match thread::Builder::new()
            .name("cortex-keepalive".into())
            .spawn(move || keepalive_loop(ka_device, ka_shared))
        {
            Ok(handle) => handle,
            Err(error) => {
                shared.running.store(false, Ordering::Relaxed);
                let _ = rx_handle.join();
                drop(device.lock().unwrap().take());
                return Err(crate::Error::Session(format!(
                    "failed to spawn keepalive thread: {error}"
                )));
            }
        };

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
    /// Currently always returns `Ok` because this Quad session swallows write
    /// errors per that device's benign STALL.
    pub fn send(&self, message_type: MessageType, payload: &[u8]) -> crate::Result<()> {
        self.send_many([(message_type, payload)])
    }

    /// Send several logical messages without allowing another sender or the
    /// keepalive thread to interleave between them.
    ///
    /// Use this for device operations whose meaning depends on message order,
    /// such as promote, switch scene, then write value. Each logical message
    /// is framed independently, but the existing write lock is retained across
    /// the complete sequence.
    ///
    /// # Errors
    ///
    /// Returns a session error if the session is closed.
    pub fn send_many<'a>(
        &self,
        messages: impl IntoIterator<Item = (MessageType, &'a [u8])>,
    ) -> crate::Result<()> {
        let reports = messages
            .into_iter()
            .map(|(message_type, payload)| encode_message(message_type as u16, payload))
            .collect::<Vec<_>>();
        let _lock = self.shared.write_lock.lock().unwrap();
        if !self.shared.running.load(Ordering::Acquire) {
            return Err(crate::Error::Session("session is closed".into()));
        }
        // Announce the intent to write BEFORE contending for the device lock,
        // so the RX loop stops reacquiring it and this write gets through.
        let _intent = WriteIntent::new(&self.shared.writers_waiting);
        let device = self.device.lock().unwrap();
        let device = device
            .as_ref()
            .ok_or_else(|| crate::Error::Session("session is closed".into()))?;
        for message_reports in &reports {
            for report in message_reports {
                let _ = device.write(report);
            }
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
            if !self.shared.running.load(Ordering::Acquire) {
                return WaitOutcome::Closed;
            }
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
        if let Err(error) = self.send(message_type, payload) {
            self.shared.pending.lock().unwrap().remove(&request_id);
            return Err(error);
        }

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
            WaitOutcome::Closed => {
                self.shared.pending.lock().unwrap().remove(&request_id);
                return Err(crate::Error::Session("session is closed".into()));
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
        trigger: impl FnOnce() -> crate::Result<()>,
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
        if let Err(error) = trigger() {
            let mut waiters = self.shared.type_waiters.lock().unwrap();
            waiters.retain(|w| !Arc::ptr_eq(w, &waiter));
            return Err(error);
        }

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
                WaitOutcome::Closed => Err(crate::Error::Session("session is closed".into())),
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
    /// Note this blocks for the full `window` while the session is open: there is no
    /// total-count field on the wire, so "have we got them all?" is
    /// unanswerable. Callers that can define completion in domain terms
    /// should poll instead (see the client's `wait_for_listing`).
    ///
    /// See spec/140-session/spec.md [FR-11].
    ///
    /// # Errors
    ///
    /// Returns a session error if the trigger fails or the session closes.
    /// An empty `Vec` means nothing matched, which is a domain-level outcome
    /// rather than a transport failure.
    pub fn collect(
        &self,
        expected_type: MessageType,
        trigger: impl FnOnce() -> crate::Result<()>,
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
        if let Err(error) = trigger() {
            self.shared
                .collectors
                .lock()
                .unwrap()
                .retain(|c| !Arc::ptr_eq(c, &collector));
            return Err(error);
        }

        let deadline = Instant::now() + window;
        while Instant::now() < deadline {
            if !self.shared.running.load(Ordering::Acquire) {
                self.shared
                    .collectors
                    .lock()
                    .unwrap()
                    .retain(|c| !Arc::ptr_eq(c, &collector));
                return Err(crate::Error::Session("session is closed".into()));
            }
            thread::sleep(SILENCE_POLL.min(deadline.saturating_duration_since(Instant::now())));
        }

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
        trace!("handshake 2/8: Version READ");
        let read = encode_read_message(MessageType::Version);
        match self.await_broadcast(
            MessageType::Version,
            || self.send(MessageType::Version, &read),
            timeout,
            |m| !m.body.is_empty(),
        ) {
            Ok(reply) => match prost::Message::decode(reply.body.as_ref()) {
                Ok(v) => {
                    let v: VersionMessage = v;
                    trace!("handshake 2/8: device version received");
                    let _ = v;
                }
                Err(e) => trace!("handshake 2/8: version undecodable: {e}"),
            },
            // Not fatal. The identity is a convenience; the handshake's job
            // is to get the device pushing, and it can do that without this.
            Err(e) => trace!("handshake 2/8: no version reply ({e})"),
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
            || self.send(MessageType::ModelRepo, payload),
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
        trace!("handshake 1/8: ResetCommsBuffers (rid={rid})");
        let _ = self.request(MessageType::ResetCommsBuffers, &payload, rid, timeout)?;
        trace!("handshake 1/8: reply received");

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
        trace!("handshake 3/8: Version UPDATE announce");
        self.send(MessageType::Version, &payload)?;

        // 4. ModelRepo READ.
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
        // Earlier this was blamed on the device building the catalog. A wire
        // trace disproved that: our reader starved our writer, and flooding
        // the handshake queued requests against this transfer. With writer
        // priority and pacing, Cortex Control and this client both receive the
        // catalog in about 0.65 s.
        progress("requesting model catalog");
        self.fetch_catalog(&payload, timeout);

        // 5. Connection{connected: true}.
        let conn_msg = ConnectionMessage {
            request_id: None,
            connected: Some(crate::proto::connection_message::Connected::Connected(true)),
        };
        let payload = prost::Message::encode_to_vec(&conn_msg);
        progress("announcing connection");
        trace!("handshake 5/8: Connection connected=true");
        self.send(MessageType::Connection, &payload)?;

        // 6. 22 subscribe READs.
        if mode == ConnectMode::Minimal {
            // Stop here. The subscription is what provokes the state dump,
            // and a command-only session has nothing to do with it.
            trace!("handshake: minimal, skipping subscription and settle");
            return Ok(());
        }

        self.shared.state.begin_subscription(self.shared.generation);
        progress("subscribing to device state");
        trace!("handshake 6/8: {} subscribe READs", SUBSCRIBE_TYPES.len());
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
        trace!("handshake 7/8: CpuLoad CREATE (rid={rid})");
        let payload = prost::Message::encode_to_vec(&cpu);
        self.send(MessageType::CpuLoad, &payload)?;

        // 8. Settle until the device stops talking.
        //
        // A fixed sleep is the wrong shape here. Subscription replies and
        // state pushes arrive asynchronously, and a caller that reads before
        // that burst settles can queue its reply behind them. The large
        // ModelRepo transfer is not part of this burst: step 4 already waited
        // for it before the subscriptions were sent.
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
        trace!("handshake 8/8: device quiet, handshake complete");
        self.shared
            .state
            .finish_subscription(self.shared.generation);

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
        self.shared.heard_anything.load(Ordering::Relaxed)
    }

    /// Whether the device has spoken recently enough for cached state to be
    /// served as live.
    ///
    /// Uses the same measured threshold as request fail-fast. A cache hit must
    /// not hide an unplug merely because it needs no wire round trip.
    #[must_use]
    pub fn is_responsive(&self) -> bool {
        self.shared.running.load(Ordering::Acquire)
            && self.has_heard_from_device()
            && self.seconds_since_last_message() < SILENCE_LIMIT.as_secs()
    }

    /// The most recent CPU load pushed by the device, if any has arrived.
    ///
    /// Only populated on a SUBSCRIBED session: the device pushes this because
    /// the handshake asked it to, and a `Minimal` session never does.
    #[must_use]
    pub fn cpu_load(&self) -> Option<CpuLoadMessage> {
        self.shared.state.cpu_load().map(|cached| cached.value)
    }

    /// The device's `Version` as read during the handshake, if it answered.
    ///
    /// `None` on a session that never handshook - `Version` is answered
    /// without one, so those sessions exist - or if the device did not reply.
    #[must_use]
    pub fn device_version(&self) -> Option<VersionMessage> {
        self.shared
            .state
            .device_version()
            .map(|cached| cached.value)
    }

    /// The `ModelRepo` payload captured during this session, if one has
    /// arrived.
    ///
    /// The handshake requests it, so by the time a caller wants the catalog
    /// it has usually already been received. Returning it here avoids a
    /// second transfer of the same 46 KB payload.
    #[must_use]
    pub fn captured_model_repo(&self) -> Option<Vec<u8>> {
        self.shared.state.model_repo().map(|cached| cached.value)
    }

    /// Clone the non-consuming subscribed-state cache handle.
    #[must_use]
    pub fn state_cache(&self) -> DeviceStateCache {
        self.shared.state.clone()
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

    /// Announce disconnect, stop workers, and destroy the owned device link.
    ///
    /// Returning guarantees that the HID handle has been dropped even while
    /// other `Arc<Session>` references still exist. Calling this more than
    /// once is harmless.
    pub fn close(&self) {
        {
            let _write = self.shared.write_lock.lock().unwrap();
            if self.shared.running.load(Ordering::Acquire) {
                let message = ConnectionMessage {
                    request_id: None,
                    connected: Some(crate::proto::connection_message::Connected::Connected(
                        false,
                    )),
                };
                let reports = encode_message(
                    MessageType::Connection as u16,
                    &prost::Message::encode_to_vec(&message),
                );
                let _intent = WriteIntent::new(&self.shared.writers_waiting);
                if let Some(device) = self.device.lock().unwrap().as_ref() {
                    for report in &reports {
                        let _ = device.write(report);
                    }
                }
                self.shared.running.store(false, Ordering::Release);
            }
        }
        self.stop();
        drop(self.device.lock().unwrap().take());
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
        self.close();
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
fn rx_loop(device: Arc<Mutex<Option<Box<dyn crate::link::HidLink>>>>, shared: Arc<Shared>) {
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
            let Some(device) = device.as_ref() else {
                break;
            };
            if let Ok(n) = device.read_timeout(&mut buf, timeout_ms) {
                n
            } else {
                reassembler.reset();
                report_count = 0;
                shared
                    .state
                    .stream_gap(shared.generation, "device read failed");
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
            shared
                .state
                .stream_gap(shared.generation, "malformed HID report");
            continue;
        };

        // A FIRST frame always begins a new message; drop any stale partial.
        if frame.flags.is_first() && report_count > 0 {
            reassembler.reset();
            report_count = 0;
            shared.state.stream_gap(
                shared.generation,
                "new FIRST frame abandoned an incomplete message",
            );
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
                // A LAST/COMPLETE frame can push the body over the cap in
                // one step; check the exact byte count before decoding
                // rather than only catching a wedged partial (below).
                if body.len() > MAX_MESSAGE_BODY {
                    shared
                        .state
                        .stream_gap(shared.generation, "reassembled message exceeded cap");
                } else if let Err(e) = handle_message(&body, &shared) {
                    // Routine, not exceptional: the device pushes types we do
                    // not decode (License and CloudLogin carry non-protobuf
                    // bodies). An envelope decode failure is different: its
                    // type is unrecoverable, so cache continuity is lost.
                    trace!("undecodable inbound message: {e}");
                    shared
                        .state
                        .stream_gap(shared.generation, "malformed message envelope");
                }
            }
            Ok(None) => {
                if report_count == 0 {
                    partial_started = Instant::now();
                }
                report_count += 1;
                // Byte-exact, not report-count-based: a report is never
                // obliged to carry a full `CHUNK_SIZE` chunk, so counting
                // reports against an assumed chunk size rejected legitimate
                // bodies built from many short reports well under the cap.
                if reassembler.buffered_len() > MAX_MESSAGE_BODY {
                    reassembler.reset();
                    report_count = 0;
                    shared
                        .state
                        .stream_gap(shared.generation, "reassembled message exceeded cap");
                }
            }
            Err(_) => {
                reassembler.reset();
                report_count = 0;
                shared
                    .state
                    .stream_gap(shared.generation, "invalid frame sequence");
            }
        }
    }
}

/// Decode a reassembled body, decompress if needed, and dispatch to waiters.
fn handle_message(body: &[u8], shared: &Shared) -> crate::Result<()> {
    // Trailer first, then gzip. Shared with the offline decoder so the two
    // cannot drift - see `Message::decode`.
    let (msg, _gzipped) = crate::message::Message::decode(body)?;

    let mt = match MessageType::try_from(i32::from(msg.message_type)) {
        Ok(MessageType::Undefined | MessageType::NumberOfMessageTypes) => {
            trace!("skipping sentinel message type {}", msg.message_type);
            return Ok(());
        }
        Ok(message_type) => message_type,
        Err(_) => {
            trace!("skipping unknown message type {}", msg.message_type);
            // The envelope proves that the device is alive, but an unknown
            // operation may have changed any state this schema knows about.
            // Count the traffic for liveness while refusing cache continuity.
            shared.heard_anything.store(true, Ordering::Relaxed);
            shared.last_any_inbound_ms.store(
                u64::try_from(shared.started.elapsed().as_millis()).unwrap_or(u64::MAX),
                Ordering::Relaxed,
            );
            shared.state.stream_gap(
                shared.generation,
                &format!("unknown message type {}", msg.message_type),
            );
            return Ok(());
        }
    };
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
    // Liveness counts everything: a heartbeat is exactly what tells us the
    // link is still up.
    shared.heard_anything.store(true, Ordering::Relaxed);
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
    // State observes before any waiter can consume and return from this
    // message. A successful explicit read therefore also repairs the cache.
    shared
        .state
        .observe(shared.generation, msg.message_type, &msg.body);
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
        if let Some(rid) = msg.request_id
            && let Some(waiter) = pending.get(&rid)
            && waiter.expected_type == msg.message_type
        {
            matched = Some(pending.remove(&rid).unwrap());
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
fn keepalive_loop(device: Arc<Mutex<Option<Box<dyn crate::link::HidLink>>>>, shared: Arc<Shared>) {
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
        let Some(device) = device.as_ref() else {
            return;
        };
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

    #[cfg(feature = "hid")]
    #[test]
    fn quad_session_rejects_nano_before_usb_enumeration() {
        let Err(error) = Session::open(DeviceKind::NanoCortex) else {
            panic!("Nano must not enter the Quad session");
        };
        assert!(matches!(
            error,
            crate::Error::UnsupportedDeviceOperation {
                device: DeviceKind::NanoCortex,
                operation: "Quad Cortex session"
            }
        ));
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
        let state = DeviceStateCache::new();
        let generation = state.begin_generation();
        Shared {
            pending: Mutex::new(HashMap::new()),
            type_waiters: Mutex::new(Vec::new()),
            collectors: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            running: AtomicBool::new(true),
            write_lock: Mutex::new(()),
            writers_waiting: AtomicUsize::new(0),
            last_inbound_ms: AtomicU64::new(0),
            heard_anything: AtomicBool::new(false),
            last_any_inbound_ms: AtomicU64::new(0),
            started: Instant::now(),
            state,
            generation,
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

    #[test]
    fn a_waiter_and_the_state_cache_both_observe_the_same_message() {
        let s = shared();
        let waiter = expect(&s, 7, MessageType::Scene);
        let scene = crate::proto::SceneMessage {
            action: MessageAction::Update as i32,
            request_id: Some(crate::proto::scene_message::RequestId::RequestId(7)),
            selected_scene: Some(crate::proto::scene_message::SelectedScene::SelectedScene(3)),
        };
        dispatch(
            &InboundMessage {
                message_type: MessageType::Scene,
                body: bytes::Bytes::from(prost::Message::encode_to_vec(&scene)),
                request_id: Some(7),
            },
            &s,
        );

        assert_eq!(delivered(&waiter), Some(MessageType::Scene));
        assert_eq!(s.state.active_scene().unwrap().value, 3);
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

#[cfg(test)]
mod link_tests {
    //! Tests over a fake [`HidLink`](crate::link::HidLink).
    //!
    //! These exist because the RX loop, the writer gate and the keepalive
    //! were previously unreachable without hardware - and every one of them
    //! has already shipped a bug that cost a day to find on real hardware.
    //! Each test below pins one of those.

    use super::*;
    use crate::link::FakeLink;

    struct DropTrackedLink {
        inner: FakeLink,
        dropped: Arc<AtomicBool>,
    }

    impl crate::link::HidLink for DropTrackedLink {
        fn write(&self, report: &[u8]) -> crate::Result<usize> {
            self.inner.write(report)
        }

        fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> crate::Result<usize> {
            self.inner.read_timeout(buf, timeout_ms)
        }
    }

    impl Drop for DropTrackedLink {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    #[test]
    fn close_releases_the_link_while_other_session_references_exist() {
        let dropped = Arc::new(AtomicBool::new(false));
        let session = Arc::new(
            Session::over(DropTrackedLink {
                inner: FakeLink::new(),
                dropped: dropped.clone(),
            })
            .unwrap(),
        );
        let retained = session.clone();
        session.close();
        assert!(dropped.load(Ordering::Acquire));
        assert!(retained.send(MessageType::KeepAlive, &[]).is_err());
    }

    #[test]
    fn close_terminates_an_existing_broadcast_wait() {
        let session = Arc::new(session_over(&FakeLink::new()));
        let waiting = session.clone();
        let (registered_tx, registered_rx) = std::sync::mpsc::channel();
        let waiter = thread::spawn(move || {
            waiting.await_broadcast(
                MessageType::GlobalTempo,
                || {
                    registered_tx.send(()).unwrap();
                    Ok(())
                },
                Duration::from_secs(30),
                |_| true,
            )
        });
        registered_rx.recv().unwrap();
        session.close();
        assert!(matches!(
            waiter.join().unwrap(),
            Err(crate::Error::Session(_))
        ));
    }

    #[test]
    fn close_terminates_an_existing_collection() {
        let session = Arc::new(session_over(&FakeLink::new()));
        let collecting = session.clone();
        let (registered_tx, registered_rx) = std::sync::mpsc::channel();
        let collector = thread::spawn(move || {
            collecting.collect(
                MessageType::File,
                || {
                    registered_tx.send(()).unwrap();
                    Ok(())
                },
                Duration::from_secs(30),
                |_| true,
            )
        });
        registered_rx.recv().unwrap();
        session.close();
        assert!(matches!(
            collector.join().unwrap(),
            Err(crate::Error::Session(_))
        ));
    }

    /// One complete inbound report carrying `body` plus the 8-byte trailer.
    fn inbound(message_type: u16, body: &[u8]) -> Vec<u8> {
        let mut msg = body.to_vec();
        msg.extend_from_slice(&message_type.to_le_bytes());
        msg.extend_from_slice(&[0u8; 6]);
        // [report id 0x01][len][flags FIRST|LAST][data]
        let mut report = vec![0x01, u8::try_from(msg.len()).unwrap(), 0xC0];
        report.extend_from_slice(&msg);
        report
    }

    fn session_over(link: &FakeLink) -> Session {
        Session::over(link.clone()).expect("session over a fake link")
    }

    /// The bug that cost the most: a reader holding the device lock across a
    /// blocking read, reacquiring it the instant it lets go, starves a
    /// foreground write. On hardware that showed as 46.8 s of silent bus in
    /// the middle of a handshake, on a device answering in 217 us.
    ///
    /// The fake is saturated so every read returns immediately, which is the
    /// only condition under which the race is lost - a link that ever went
    /// quiet would let the writer through by luck.
    #[test]
    fn a_waiting_writer_is_not_starved_by_the_rx_loop() {
        let link = FakeLink::new().saturated(inbound(MessageType::GlobalTempo as u16, &[]));
        let session = session_over(&link);
        // Let the RX loop reach its steady state before contending with it.
        thread::sleep(Duration::from_millis(100));

        let before = link.write_count();
        let start = Instant::now();
        session.send(MessageType::KeepAlive, &[]).unwrap();
        let waited = start.elapsed();
        session.stop();

        assert!(
            link.write_count() > before,
            "the write never reached the device at all"
        );
        assert!(
            waited < Duration::from_secs(5),
            "a write waited {waited:?} behind the RX loop - the writer gate is not holding it off"
        );
    }

    /// The RX loop must reassemble a frame and deliver it to a waiter.
    #[test]
    fn a_reassembled_message_reaches_a_waiter() {
        let link = FakeLink::new();
        let session = session_over(&link);

        let got = session.await_broadcast(
            MessageType::GlobalTempo,
            || {
                link.push_inbound(inbound(MessageType::GlobalTempo as u16, &[]));
                Ok(())
            },
            Duration::from_secs(3),
            |_| true,
        );
        session.stop();
        assert!(got.is_ok(), "a queued report never reached the waiter");
    }

    /// Keepalives must go out about once a second. At 5 s the device stops
    /// pushing state entirely, which is what made healthy sessions look dead.
    #[test]
    fn keepalives_go_out_about_once_a_second() {
        let link = FakeLink::new();
        let session = session_over(&link);
        thread::sleep(Duration::from_millis(2_500));
        let count = link.write_count();
        session.stop();

        assert!(
            count >= 2,
            "only {count} keepalives in 2.5s; the interval has drifted above 1s, \
             and the device stops pushing when they are too sparse"
        );
    }

    /// Silence is only meaningful once the device has spoken. Before the
    /// first message there is nothing to compare against, and a health check
    /// that missed this would condemn every session in its opening seconds.
    #[test]
    fn silence_means_nothing_until_the_device_has_spoken() {
        let link = FakeLink::new();
        let session = session_over(&link);
        assert!(
            !session.has_heard_from_device(),
            "a session that has received nothing must not claim it has"
        );

        link.push_inbound(inbound(MessageType::GlobalTempo as u16, &[]));
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && !session.has_heard_from_device() {
            thread::sleep(Duration::from_millis(20));
        }
        let heard = session.has_heard_from_device();
        session.stop();
        assert!(heard, "an inbound message did not register as contact");
    }

    #[test]
    fn a_valid_unknown_message_counts_as_liveness_and_invalidates_the_cache() {
        let link = FakeLink::new();
        let session = session_over(&link);
        link.push_inbound(inbound(72, &[]));

        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline
            && (!session.has_heard_from_device()
                || session.state_cache().status().phase != crate::CachePhase::Invalidated)
        {
            thread::sleep(Duration::from_millis(20));
        }
        let heard = session.has_heard_from_device();
        let status = session.state_cache().status();
        session.stop();

        assert!(heard, "valid unknown traffic must count as device contact");
        assert_eq!(status.phase, crate::CachePhase::Invalidated);
        assert_eq!(status.counters.stream_gaps, 1);
    }

    /// A malformed report must not wedge the loop or be mistaken for a
    /// message.
    #[test]
    fn a_malformed_report_is_skipped_without_stopping_the_loop() {
        let link = FakeLink::new();
        let session = session_over(&link);

        link.push_inbound(vec![0xFF, 0xFF, 0xFF]); // not our framing
        let got = session.await_broadcast(
            MessageType::GlobalTempo,
            || {
                link.push_inbound(inbound(MessageType::GlobalTempo as u16, &[]));
                Ok(())
            },
            Duration::from_secs(3),
            |_| true,
        );
        let cache_phase = session.state_cache().status().phase;
        session.stop();
        assert!(
            got.is_ok(),
            "a bad report stopped the loop delivering good ones"
        );
        assert_eq!(
            cache_phase,
            crate::CachePhase::Invalidated,
            "skipping an unknown report without invalidating would retain possibly stale state"
        );
    }

    #[test]
    fn a_malformed_message_envelope_invalidates_cache_without_stopping_the_loop() {
        let link = FakeLink::new();
        let session = session_over(&link);

        // Valid HID framing but no 8-byte envelope trailer.
        link.push_inbound(vec![0x01, 3, 0xC0, 0x01, 0x02, 0x03]);
        let got = session.await_broadcast(
            MessageType::GlobalTempo,
            || {
                link.push_inbound(inbound(MessageType::GlobalTempo as u16, &[]));
                Ok(())
            },
            Duration::from_secs(3),
            |_| true,
        );
        let cache_phase = session.state_cache().status().phase;
        session.stop();

        assert!(
            got.is_ok(),
            "a bad envelope stopped the loop delivering good ones"
        );
        assert_eq!(cache_phase, crate::CachePhase::Invalidated);
    }

    /// Frame a message as reports carrying exactly `chunk_len` data bytes
    /// each, regardless of the device's real per-report capacity.
    ///
    /// [`crate::framing::encode_reports`] always fills each report to a
    /// fixed geometry's capacity, which cannot reproduce a device sending
    /// many reports that carry far less than they are allowed to - the
    /// exact shape that exposed the report-count cap-check bug.
    fn short_chunk_reports(msg: &[u8], chunk_len: usize) -> Vec<Vec<u8>> {
        assert!(chunk_len > 0);
        let chunks: Vec<&[u8]> = msg.chunks(chunk_len).collect();
        chunks
            .iter()
            .enumerate()
            .map(|(i, chunk)| {
                let is_first = i == 0;
                let is_last = i == chunks.len() - 1;
                let flags = match (is_first, is_last) {
                    (true, true) => 0xC0,
                    (true, false) => 0x40,
                    (false, true) => 0x80,
                    (false, false) => 0x00,
                };
                let mut report = vec![0x01, u8::try_from(chunk.len()).unwrap(), flags];
                report.extend_from_slice(chunk);
                report
            })
            .collect()
    }

    /// A `GlobalTempo`-tagged body of `len` zero bytes (the last 8 forming
    /// the trailer), which passes envelope decode and correlation without
    /// exercising the state reducer (`GlobalTempo` is a heartbeat type the
    /// reducer ignores), keeping these cap tests focused on the RX loop.
    fn zero_body(len: usize) -> Vec<u8> {
        assert!(len >= 8);
        let mut msg = vec![0u8; len - 8];
        msg.extend_from_slice(&(MessageType::GlobalTempo as u16).to_le_bytes());
        msg.extend_from_slice(&[0u8; 6]);
        msg
    }

    fn wait_for_stream_gap(session: &Session) -> crate::DeviceStateStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = session.state_cache().status();
            if status.counters.stream_gaps > 0 {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for the RX loop to record a stream gap"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// [PROT-005.12] A FIRST frame abandoning a partial message must count
    /// as a stream gap and invalidate continuity, but the RX loop must
    /// still reassemble and deliver the next complete message rather than
    /// wedging on the abandoned bytes.
    #[test]
    fn a_first_frame_abandoning_a_partial_message_invalidates_continuity_and_still_delivers_the_next()
     {
        let link = FakeLink::new();
        let session = session_over(&link);

        // FIRST + MIDDLE with no LAST: an abandoned partial message.
        link.push_inbound(vec![0x01, 1, 0x40, 0xAA]);
        link.push_inbound(vec![0x01, 1, 0x00, 0xBB]);

        let got = session.await_broadcast(
            MessageType::GlobalTempo,
            || {
                // This FIRST must discard the stale partial above rather
                // than append to it or refuse to start a new message.
                link.push_inbound(inbound(MessageType::GlobalTempo as u16, &[]));
                Ok(())
            },
            Duration::from_secs(3),
            |_| true,
        );
        let status = session.state_cache().status();
        session.stop();

        assert!(
            got.is_ok(),
            "a FIRST frame abandoning a partial message must still deliver the next valid one"
        );
        assert_eq!(status.phase, crate::CachePhase::Invalidated);
        assert!(
            status.counters.stream_gaps >= 1,
            "abandoning a partial message must be counted as a stream gap"
        );
    }

    /// [PROT-005.12] The cap must be enforced against actual buffered
    /// bytes, not `report_count * an assumed per-report size`: a body built
    /// from many reports that each carry far less than the device's real
    /// per-report capacity must not be rejected while still well under the
    /// 1 MiB cap.
    #[test]
    fn many_short_reports_well_under_the_cap_are_not_rejected() {
        let link = FakeLink::new();
        let session = session_over(&link);

        // 9,000 one-byte reports: more reports than `1 MiB / CHUNK_SIZE`
        // (8,322) would ever allow under a report-count approximation, but
        // under 9 KB of actual buffered data - nowhere near the 1 MiB cap.
        let msg = zero_body(9_000);
        assert!(msg.len() > MAX_MESSAGE_BODY / crate::framing::CHUNK_SIZE);

        let got = session.await_broadcast(
            MessageType::GlobalTempo,
            || {
                for report in short_chunk_reports(&msg, 1) {
                    link.push_inbound(report);
                }
                Ok(())
            },
            Duration::from_secs(10),
            |_| true,
        );
        let status = session.state_cache().status();
        session.stop();

        assert!(
            got.is_ok(),
            "a body under the cap must not be rejected solely because it used many short reports"
        );
        assert_eq!(
            status.counters.stream_gaps, 0,
            "no stream gap should be recorded for a body under the cap"
        );
    }

    /// [PROT-005.12] A body of exactly `MAX_MESSAGE_BODY` bytes must not be
    /// rejected: the cap excludes bodies strictly larger than the limit,
    /// not bodies at it.
    #[test]
    fn a_body_at_exactly_the_byte_cap_is_not_rejected() {
        let link = FakeLink::new();
        let session = session_over(&link);
        let msg = zero_body(MAX_MESSAGE_BODY);

        let got = session.await_broadcast(
            MessageType::GlobalTempo,
            || {
                for report in crate::framing::encode_reports(
                    crate::framing::HidReportGeometry::QUAD_CORTEX,
                    &msg,
                ) {
                    link.push_inbound(report);
                }
                Ok(())
            },
            Duration::from_secs(10),
            |_| true,
        );
        let status = session.state_cache().status();
        session.stop();

        assert!(got.is_ok(), "a body exactly at the cap must be accepted");
        assert_eq!(status.counters.stream_gaps, 0);
    }

    /// [PROT-005.12] A partial body crossing the cap before LAST must reset
    /// immediately, without waiting for a completing frame, and the RX loop
    /// must recover for the next FIRST-delimited message.
    #[test]
    fn an_in_progress_body_over_the_cap_is_reset_and_the_loop_recovers() {
        let link = FakeLink::new();
        let session = session_over(&link);
        let oversized = zero_body(MAX_MESSAGE_BODY + 1);
        let mut reports = crate::framing::encode_reports(
            crate::framing::HidReportGeometry::QUAD_CORTEX,
            &oversized,
        );
        // Keep the final chunk in-progress so this exercises the partial-body
        // branch rather than the completed-body check below.
        reports.last_mut().unwrap()[2] = 0x00;

        for report in reports {
            link.push_inbound(report);
        }
        let status_after_oversized = wait_for_stream_gap(&session);

        assert_eq!(status_after_oversized.counters.stream_gaps, 1);
        assert_eq!(status_after_oversized.phase, crate::CachePhase::Invalidated);
        assert_eq!(
            status_after_oversized.last_rejection.as_deref(),
            Some("reassembled message exceeded cap")
        );
        assert!(
            !session.has_heard_from_device(),
            "an oversized partial must not be dispatched as device traffic"
        );

        let got = session.await_broadcast(
            MessageType::GlobalTempo,
            || {
                link.push_inbound(inbound(MessageType::GlobalTempo as u16, &[]));
                Ok(())
            },
            Duration::from_secs(3),
            |_| true,
        );
        session.stop();

        assert!(
            got.is_ok(),
            "the RX loop must recover after an in-progress body breaches the cap"
        );
    }

    /// [PROT-005.12] A LAST frame that takes the reassembled body one byte
    /// over the cap must be rejected as a stream gap before envelope
    /// decoding is attempted, and the RX loop must recover to deliver the
    /// next valid message rather than wedging.
    #[test]
    fn a_last_frame_over_the_cap_is_rejected_before_decoding_and_the_loop_recovers() {
        let link = FakeLink::new();
        let session = session_over(&link);
        let mut oversized = zero_body(MAX_MESSAGE_BODY + 1);
        // If decoding moves ahead of the cap check, this invalid gzip prefix
        // changes the rejection reason and the test catches the ordering bug.
        oversized[..2].copy_from_slice(&[0x1f, 0x8b]);

        for report in crate::framing::encode_reports(
            crate::framing::HidReportGeometry::QUAD_CORTEX,
            &oversized,
        ) {
            link.push_inbound(report);
        }
        let status_after_oversized = wait_for_stream_gap(&session);

        assert_eq!(status_after_oversized.counters.stream_gaps, 1);
        assert_eq!(status_after_oversized.phase, crate::CachePhase::Invalidated);
        assert_eq!(
            status_after_oversized.last_rejection.as_deref(),
            Some("reassembled message exceeded cap")
        );
        assert!(
            !session.has_heard_from_device(),
            "an over-cap completed body must be rejected before dispatch"
        );

        let got = session.await_broadcast(
            MessageType::GlobalTempo,
            || {
                link.push_inbound(inbound(MessageType::GlobalTempo as u16, &[]));
                Ok(())
            },
            Duration::from_secs(3),
            |_| true,
        );
        session.stop();

        assert!(
            got.is_ok(),
            "the RX loop must recover and deliver the next valid message after a cap breach"
        );
    }
}
