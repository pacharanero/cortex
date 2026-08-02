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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use crate::DeviceKind;
use crate::framing::{Frame, FrameReassembler, encode_message};
use crate::proto::cortex_message_type::Enum as MessageType;
use crate::proto::message_action::Enum as MessageAction;
use crate::proto::reset_comms_buffers_message::SessionId as RcbSessionId;
use crate::proto::{
    ConnectionMessage, KeepAliveMessage, ModelRepoMessage, ResetCommsBuffersMessage, VersionMessage,
};
use crate::transport::{DEFAULT_READ_TIMEOUT, HID_REPORT_LEN};

/// The Cortex Control version string the host announces to the device.
/// The device gates state-push behaviour on receiving a valid CC version.
/// Captured on the wire against `CorOS` 4.0.1.
const CC_VERSION: &str = "4.0.1";

/// Default keepalive interval. Cortex Control pings every ~5 seconds.
const DEFAULT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);

/// After a request wait times out, a reply can still land in the race window.
/// Wait this long for the RX thread to finish delivering before declaring a
/// timeout.
const DELIVERY_GRACE: Duration = Duration::from_millis(500);

/// Hard upper bound on reassembled body size. A legitimate message never
/// reaches this (the `ModelRepo` reply, ~47 KB, is the largest observed).
const MAX_MESSAGE_BODY: usize = 1 << 20; // 1 MiB

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
        let device = self.device.lock().unwrap();
        for report in &reports {
            let _ = device.write(report);
        }
        Ok(())
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

        // Wait for the event or timeout.
        let (lock, cvar) = &*event;
        let result = lock.lock().unwrap();
        if *result {
            drop(result);
        } else {
            let (result, timeout_res) = cvar.wait_timeout(result, timeout).unwrap();
            if !*result {
                drop(result);
                // Timed out. Check if the RX thread already popped the entry.
                let delivered_by_rx = self
                    .shared
                    .pending
                    .lock()
                    .unwrap()
                    .remove(&request_id)
                    .is_none();
                if delivered_by_rx {
                    // The RX thread is committed to delivering; wait a grace.
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
                let _ = timeout_res;
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

        // Wait.
        let (lock, cvar) = &*event;
        let result = lock.lock().unwrap();
        if *result {
            drop(result);
        } else {
            let (result, _) = cvar.wait_timeout(result, timeout).unwrap();
            if !*result {
                drop(result);
                // Timed out; remove the waiter.
                let mut waiters = self.shared.type_waiters.lock().unwrap();
                waiters.retain(|w| !Arc::ptr_eq(w, &waiter));
                if waiter.slot.lock().unwrap().is_some() {
                    return Ok(waiter.slot.lock().unwrap().take().unwrap());
                }
                return Err(crate::Error::ReadTimeout(timeout));
            }
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
        // 1. ResetCommsBuffers with a fresh session_id.
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        let rid = self.next_request_id();
        let msg = ResetCommsBuffersMessage {
            session_id: Some(RcbSessionId::SessionId(session_id)),
            request_id: Some(crate::proto::reset_comms_buffers_message::RequestId::RequestId(rid)),
        };
        let payload = prost::Message::encode_to_vec(&msg);
        let _ = self.request(MessageType::ResetCommsBuffers, &payload, rid, timeout)?;

        // 2. Version UPDATE announcing cortex_control_version.
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
        self.send(MessageType::Version, &payload)?;

        // 3. ModelRepo READ.
        let repo_msg = ModelRepoMessage {
            action: MessageAction::Read as i32,
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&repo_msg);
        self.send(MessageType::ModelRepo, &payload)?;

        // 4. Connection{connected: true}.
        let conn_msg = ConnectionMessage {
            request_id: None,
            connected: Some(crate::proto::connection_message::Connected::Connected(true)),
        };
        let payload = prost::Message::encode_to_vec(&conn_msg);
        self.send(MessageType::Connection, &payload)?;

        // 5. 22 subscribe READs.
        for mt in SUBSCRIBE_TYPES {
            let payload = encode_read_message(*mt);
            self.send(*mt, &payload)?;
        }

        // 6. Settle.
        thread::sleep(settle);

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

/// The background RX loop: read frames, reassemble, decode, dispatch.
fn rx_loop(device: Arc<Mutex<hidapi::HidDevice>>, shared: Arc<Shared>) {
    let mut reassembler = FrameReassembler::new();
    let mut report_count: usize = 0;

    while shared.running.load(Ordering::Relaxed) {
        let mut buf = vec![0u8; HID_REPORT_LEN];
        let timeout_ms = i32::try_from(DEFAULT_READ_TIMEOUT.as_millis().min(i32::MAX as u128))
            .unwrap_or(i32::MAX);
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
                report_count = 0;
                if let Err(e) = handle_message(&body, &shared) {
                    eprintln!("cortex-rx: message handling error: {e}");
                }
            }
            Ok(None) => {
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
    let mut msg = crate::message::Message::parse(body)?;

    // Frame-level gzip decompression.
    if msg.body.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut decoder = flate2::read::GzDecoder::new(&msg.body[..]);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| crate::Error::Decode(format!("gzip: {e}")))?;
        msg.body = bytes::Bytes::from(decompressed);
    }

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

/// The background keepalive loop: send KeepAlive{UPDATE} every 5 seconds.
fn keepalive_loop(device: Arc<Mutex<hidapi::HidDevice>>, shared: Arc<Shared>) {
    while shared.running.load(Ordering::Relaxed) {
        thread::sleep(DEFAULT_KEEPALIVE_INTERVAL);
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
