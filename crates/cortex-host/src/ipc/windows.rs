// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Windows named-pipe adapter behind the local IPC facade.
//!
//! @see spec/roadmap.md [CLI-004.12]
//! @see spec/200-cli/spec.md [FR-18]
//! @see spec/200-cli/design.md [DES-CLI]

#![allow(unsafe_code)]

use std::fmt::{self, Write as _};
use std::io::{Read, Write};
use std::num::NonZeroU8;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use interprocess::os::windows::named_pipe::{
    DuplexPipeStream, PipeListener, PipeListenerOptions, PipeMode, pipe_mode,
};
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use widestring::U16CString;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_BUSY, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, IsValidSid, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, ReadFile, SECURITY_ANONYMOUS,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResultEx, OVERLAPPED};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, INFINITE, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const RETRY_POLL: Duration = Duration::from_millis(1);
static PAIR_NONCE: AtomicU64 = AtomicU64::new(0);

type BytePipe = DuplexPipeStream<pipe_mode::Bytes>;
type BytePipeListener = PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>;
type ClaimPipeListener = PipeListener<pipe_mode::Bytes, pipe_mode::None>;

fn stable_hash(value: impl IntoIterator<Item = u8>) -> u64 {
    value.into_iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn user_scope() -> String {
    static SCOPE: OnceLock<String> = OnceLock::new();
    SCOPE
        .get_or_init(|| {
            let sid = current_user_sid().expect("could not query the current Windows user SID");
            format!("{:016x}", stable_hash(sid))
        })
        .clone()
}

fn current_user_sid() -> std::io::Result<Vec<u8>> {
    // SAFETY: GetCurrentProcess returns a process pseudo-handle that remains
    // valid for this process and must not be closed.
    process_user_sid(unsafe { GetCurrentProcess() })
}

fn process_user_sid(process: windows_sys::Win32::Foundation::HANDLE) -> std::io::Result<Vec<u8>> {
    let mut token = ptr::null_mut();
    // SAFETY: `token` points to writable storage; successful calls initialize
    // one owned token handle, which is immediately wrapped for RAII closure.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: OpenProcessToken returned this newly owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut byte_len = 0;
    // SAFETY: a null information buffer with length zero is the documented
    // sizing query; `byte_len` points to writable output storage.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &raw mut byte_len,
        );
    }
    if byte_len == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let word_len = usize::try_from(byte_len)
        .unwrap_or(usize::MAX)
        .div_ceil(std::mem::size_of::<usize>());
    let mut token_info = vec![0_usize; word_len];
    // SAFETY: the word-aligned allocation is at least `byte_len` bytes and
    // remains live while the returned TOKEN_USER and SID pointers are read.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            token_info.as_mut_ptr().cast(),
            byte_len,
            &raw mut byte_len,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: GetTokenInformation initialized a word-aligned TOKEN_USER at the
    // start of `token_info` and its embedded SID remains within that buffer.
    let token_user = unsafe { &*token_info.as_ptr().cast::<TOKEN_USER>() };
    let sid = token_user.User.Sid;
    // SAFETY: the SID pointer came from a successful TokenUser query.
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows returned an invalid user SID",
        ));
    }
    // SAFETY: IsValidSid accepted this SID pointer.
    let sid_len = unsafe { GetLengthSid(sid) };
    // SAFETY: GetLengthSid gives the readable byte extent of the valid SID,
    // which remains live in `token_info` until the copy completes.
    let sid = unsafe {
        std::slice::from_raw_parts(sid.cast::<u8>(), usize::try_from(sid_len).unwrap_or(0))
    };
    Ok(sid.to_vec())
}

fn process_sid(process_id: u32) -> std::io::Result<Vec<u8>> {
    // SAFETY: OpenProcess receives a numeric PID and returns either null or a
    // newly owned query-only handle, wrapped immediately for RAII closure.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: OpenProcess returned this newly owned handle.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    process_user_sid(process.as_raw_handle())
}

fn sid_string(sid: &[u8]) -> std::io::Result<String> {
    let Some((&revision, rest)) = sid.split_first() else {
        return Err(invalid_sid_data());
    };
    let Some((&subauthority_count, body)) = rest.split_first() else {
        return Err(invalid_sid_data());
    };
    let Some((authority_bytes, subauthorities)) = body.split_at_checked(6) else {
        return Err(invalid_sid_data());
    };
    if subauthorities.len() != usize::from(subauthority_count) * 4 {
        return Err(invalid_sid_data());
    }
    let authority = authority_bytes
        .iter()
        .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte));
    let mut string = format!("S-{revision}-{authority}");
    for subauthority in subauthorities.chunks_exact(4) {
        let value = u32::from_le_bytes(subauthority.try_into().expect("four-byte SID component"));
        write!(string, "-{value}").expect("writing to a String is infallible");
    }
    Ok(string)
}

fn invalid_sid_data() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "Windows returned invalid binary SID data",
    )
}

fn owner_only_security_descriptor() -> std::io::Result<SecurityDescriptor> {
    let sid = sid_string(&current_user_sid()?)?;
    let sddl = U16CString::from_str(format!("O:{sid}D:P(A;;GA;;;{sid})"))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    SecurityDescriptor::deserialize(&sddl)
}

/// Platform-specific address of the local daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEndpoint {
    name: String,
    claim_name: String,
    log_path: PathBuf,
}

impl LocalEndpoint {
    /// The current user's Cortex named-pipe endpoint.
    #[must_use]
    pub fn daemon() -> Self {
        Self::for_device(cortex_rs::DeviceKind::QuadCortex)
    }

    /// The current user's named-pipe endpoint for one Cortex product.
    #[must_use]
    pub fn for_device(device: cortex_rs::DeviceKind) -> Self {
        let suffix = match device {
            cortex_rs::DeviceKind::QuadCortex => "",
            cortex_rs::DeviceKind::NanoCortex => "-nano",
        };
        let scope = user_scope();
        Self {
            name: format!(r"\\.\pipe\cortex-{scope}{suffix}"),
            claim_name: format!(r"\\.\pipe\cortex-claim-{scope}{suffix}"),
            log_path: std::env::temp_dir().join(format!("cortex-{scope}{suffix}.log")),
        }
    }

    /// Build an endpoint around an explicit test pipe name.
    #[must_use]
    pub fn at(path: impl AsRef<Path>) -> Self {
        let supplied = path.as_ref().to_string_lossy();
        let name = if supplied.starts_with(r"\\.\pipe\") {
            supplied.into_owned()
        } else {
            format!(
                r"\\.\pipe\cortex-test-{:016x}",
                stable_hash(supplied.bytes())
            )
        };
        let hash = stable_hash(name.bytes());
        Self {
            name,
            claim_name: format!(r"\\.\pipe\cortex-claim-test-{hash:016x}"),
            log_path: std::env::temp_dir().join(format!("cortex-test-{hash:016x}.log")),
        }
    }

    /// Place for logs from the detached daemon process.
    #[must_use]
    pub fn log_path(&self) -> PathBuf {
        self.log_path.clone()
    }

    pub(crate) fn has_active_claim(&self) -> bool {
        match LocalClaim::acquire(self) {
            Ok(claim) => {
                drop(claim);
                false
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => true,
            Err(_) => true,
        }
    }
}

impl fmt::Display for LocalEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.name.fmt(formatter)
    }
}

/// Result of claiming the local daemon endpoint.
pub struct BindResult {
    /// Listener holding the endpoint claim.
    pub listener: LocalListener,
    /// Named pipes have no stale filesystem endpoint.
    pub removed_stale_endpoint: bool,
}

/// Process-lifetime ownership claim shared by daemon and direct device access.
pub struct LocalClaim {
    _listener: ClaimPipeListener,
}

impl LocalClaim {
    /// Atomically claim the current user's Cortex device ownership slot.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::AddrInUse`] when another process owns the
    /// claim, or the underlying named-pipe error.
    pub fn acquire(endpoint: &LocalEndpoint) -> std::io::Result<Self> {
        let security = owner_only_security_descriptor()?;
        let listener = PipeListenerOptions::new()
            .path(Path::new(&endpoint.claim_name))
            .mode(PipeMode::Bytes)
            .instance_limit(NonZeroU8::new(1))
            .accept_remote(false)
            .inheritable(false)
            .security_descriptor(Some(security))
            .create_recv_only::<pipe_mode::Bytes>()
            .map_err(|error| {
                let code = error
                    .raw_os_error()
                    .and_then(|code| u32::try_from(code).ok());
                if matches!(code, Some(ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY)) {
                    std::io::Error::new(
                        std::io::ErrorKind::AddrInUse,
                        format!("another process already owns {endpoint}"),
                    )
                } else {
                    error
                }
            })?;
        Ok(Self {
            _listener: listener,
        })
    }
}

/// Listener for local daemon clients.
pub struct LocalListener {
    inner: BytePipeListener,
    _claim: LocalClaim,
}

impl LocalListener {
    /// Claim a current-owner-only, local-only named pipe.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::AddrInUse`] for a live owner, or an
    /// underlying lock or named-pipe error.
    pub fn bind(endpoint: &LocalEndpoint) -> std::io::Result<BindResult> {
        let claim = LocalClaim::acquire(endpoint)?;
        let security = owner_only_security_descriptor()?;
        let inner = PipeListenerOptions::new()
            .path(Path::new(&endpoint.name))
            .mode(PipeMode::Bytes)
            .accept_remote(false)
            .inheritable(false)
            .security_descriptor(Some(security))
            .create_duplex::<pipe_mode::Bytes>()?;
        Ok(BindResult {
            listener: Self {
                inner,
                _claim: claim,
            },
            removed_stale_endpoint: false,
        })
    }

    /// Accept one local client connection.
    ///
    /// # Errors
    ///
    /// Returns the named-pipe accept error.
    pub fn accept(&self) -> std::io::Result<LocalConnection> {
        let inner = self.inner.accept()?;
        // The listener polls in PIPE_NOWAIT mode, which accepted server
        // instances inherit. Restore blocking I/O so zero bytes means EOF and
        // the connection's overlapped read timeout controls waiting.
        inner.set_nonblocking(false)?;
        Ok(LocalConnection::new(inner))
    }

    /// Configure whether accept returns immediately when no client is waiting.
    ///
    /// # Errors
    ///
    /// Returns the named-pipe mode error.
    pub fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        self.inner.set_nonblocking(nonblocking)
    }

    /// Named pipes leave no filesystem endpoint to remove.
    ///
    /// # Errors
    ///
    /// This adapter currently performs no fallible cleanup.
    pub fn cleanup_endpoint(&self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct Timeouts {
    read: Option<Duration>,
    write: Option<Duration>,
}

/// Duplex byte stream to the local daemon.
pub struct LocalConnection {
    inner: Arc<SharedPipe>,
    timeouts: Arc<Mutex<Timeouts>>,
    write_timed_out: Arc<Mutex<bool>>,
}

struct SharedPipe(BytePipe);

impl LocalConnection {
    fn new(inner: BytePipe) -> Self {
        Self {
            inner: Arc::new(SharedPipe(inner)),
            timeouts: Arc::new(Mutex::new(Timeouts::default())),
            write_timed_out: Arc::new(Mutex::new(false)),
        }
    }

    /// Connect to one endpoint.
    ///
    /// # Errors
    ///
    /// Returns the named-pipe connection error or a timeout when every server
    /// instance remains occupied.
    pub fn connect(endpoint: &LocalEndpoint) -> std::io::Result<Self> {
        let name = U16CString::from_str(&endpoint.name).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
        })?;
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        loop {
            // SECURITY_SQOS_PRESENT with SECURITY_ANONYMOUS prevents a named-
            // pipe server from impersonating this client. The returned handle
            // uses overlapped mode because interprocess's synchronous wrapper
            // performs its reads and writes through alertable overlapped I/O.
            // SAFETY: all pointers are valid for the call; null security and
            // template handles are permitted by CreateFileW.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS,
                    ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                // SAFETY: CreateFileW returned this newly owned handle.
                let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
                let pipe = BytePipe::try_from(handle).map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
                })?;
                let server_sid = process_sid(pipe.server_process_id()?)?;
                if server_sid != current_user_sid()? {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "the Cortex named-pipe server belongs to another Windows user",
                    ));
                }
                return Ok(Self::new(pipe));
            }

            let error = std::io::Error::last_os_error();
            let error_code = error
                .raw_os_error()
                .and_then(|code| u32::try_from(code).ok());
            let retryable = error_code
                .is_some_and(|code| matches!(code, ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY));
            if !retryable {
                return Err(error);
            }
            if error_code == Some(ERROR_FILE_NOT_FOUND) && !endpoint.has_active_claim() {
                return Err(error);
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("timed out connecting to {endpoint}"),
                ));
            }
            std::thread::sleep(RETRY_POLL);
        }
    }

    /// Clone the stream handle for independent buffered reading and writing.
    ///
    /// # Errors
    ///
    /// This Windows adapter shares one synchronized pipe handle and is
    /// currently infallible; the result preserves parity with the Unix facade.
    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Self {
            inner: Arc::clone(&self.inner),
            timeouts: Arc::clone(&self.timeouts),
            write_timed_out: Arc::clone(&self.write_timed_out),
        })
    }

    /// Set the read timeout.
    ///
    /// # Errors
    ///
    /// Returns an invalid-input error for a zero timeout.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        validate_timeout(timeout)?;
        let mut timeouts = self
            .timeouts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        timeouts.read = timeout;
        Ok(())
    }

    /// Set the write timeout.
    ///
    /// # Errors
    ///
    /// Returns an invalid-input error for a zero timeout.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        validate_timeout(timeout)?;
        let mut timeouts = self
            .timeouts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        timeouts.write = timeout;
        Ok(())
    }

    /// Connected pair used by transport-independent protocol tests.
    ///
    /// # Errors
    ///
    /// Returns a lock, named-pipe creation, connection, or accept error.
    #[doc(hidden)]
    pub fn pair() -> std::io::Result<(Self, Self)> {
        let unique = PAIR_NONCE.fetch_add(1, Ordering::Relaxed);
        let endpoint = LocalEndpoint::at(PathBuf::from(format!(
            r"\\.\pipe\cortex-pair-{}-{unique}",
            std::process::id()
        )));
        let bound = LocalListener::bind(&endpoint)?;
        let client = Self::connect(&endpoint)?;
        let server = bound.listener.accept()?;
        Ok((client, server))
    }

    fn read_timeout(&self) -> Option<Duration> {
        self.timeouts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .read
    }

    fn write_timeout(&self) -> Option<Duration> {
        self.timeouts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .write
    }
}

fn validate_timeout(timeout: Option<Duration>) -> std::io::Result<()> {
    if timeout.is_some_and(|timeout| timeout.is_zero()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "timeout must be greater than zero",
        ));
    }
    Ok(())
}

fn timed_overlapped_io(
    handle: HANDLE,
    timeout: Duration,
    start: impl FnOnce(*mut OVERLAPPED) -> i32,
) -> std::io::Result<usize> {
    // SAFETY: null attributes and name are permitted; the returned event is
    // immediately wrapped for RAII closure.
    let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    if event.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: CreateEventW returned this newly owned event handle.
    let event = unsafe { OwnedHandle::from_raw_handle(event) };
    let mut overlapped = OVERLAPPED {
        hEvent: event.as_raw_handle(),
        ..OVERLAPPED::default()
    };

    if start(&raw mut overlapped) == 0 {
        let error = std::io::Error::last_os_error();
        if error
            .raw_os_error()
            .and_then(|code| u32::try_from(code).ok())
            != Some(ERROR_IO_PENDING)
        {
            return Err(error);
        }
    }

    let wait_ms = u32::try_from(
        timeout
            .as_nanos()
            .div_ceil(1_000_000)
            .clamp(1, u128::from(INFINITE.saturating_sub(1))),
    )
    .expect("the timeout was clamped to a finite DWORD");
    let mut transferred = 0_u32;
    // SAFETY: the pipe, event, OVERLAPPED and transfer count remain live for
    // the full wait, and this thread does not reuse the OVERLAPPED concurrently.
    if unsafe {
        GetOverlappedResultEx(
            handle,
            &raw const overlapped,
            &raw mut transferred,
            wait_ms,
            0,
        )
    } != 0
    {
        return Ok(usize::try_from(transferred).unwrap_or(usize::MAX));
    }

    let error = std::io::Error::last_os_error();
    if error
        .raw_os_error()
        .and_then(|code| u32::try_from(code).ok())
        != Some(WAIT_TIMEOUT)
    {
        return Err(error);
    }

    // SAFETY: cancellation names this operation's live OVERLAPPED. The second
    // wait drains completion before the caller's buffer can be released.
    let cancelled = unsafe { CancelIoEx(handle, &raw const overlapped) } != 0;
    // SAFETY: the same live operation is awaited to completion without a bound
    // after cancellation, as required before its borrowed buffer can be reused.
    if unsafe {
        GetOverlappedResultEx(
            handle,
            &raw const overlapped,
            &raw mut transferred,
            INFINITE,
            0,
        )
    } != 0
    {
        // Completion won the cancellation race; report its real result so a
        // caller never retries an operation that actually reached the peer.
        return Ok(usize::try_from(transferred).unwrap_or(usize::MAX));
    }
    let cancellation_error = std::io::Error::last_os_error();
    let cancellation_code = cancellation_error
        .raw_os_error()
        .and_then(|code| u32::try_from(code).ok());
    if cancelled && cancellation_code == Some(ERROR_OPERATION_ABORTED) {
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "local named-pipe operation timed out",
        ))
    } else {
        Err(cancellation_error)
    }
}

impl Read for LocalConnection {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let inner = &self.inner.0;
        let Some(timeout) = self.read_timeout() else {
            return (&mut &*inner).read(buffer);
        };
        if buffer.is_empty() {
            return Ok(0);
        }
        let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        let result = timed_overlapped_io(inner.as_raw_handle(), timeout, |overlapped| {
            // SAFETY: the buffer is writable for `length` bytes and remains
            // borrowed until timed_overlapped_io completes or drains cancellation.
            unsafe {
                ReadFile(
                    inner.as_raw_handle(),
                    buffer.as_mut_ptr(),
                    length,
                    ptr::null_mut(),
                    overlapped,
                )
            }
        });
        match result {
            Err(error)
                if error.kind() == std::io::ErrorKind::BrokenPipe
                    || error
                        .raw_os_error()
                        .and_then(|code| u32::try_from(code).ok())
                        == Some(ERROR_PIPE_NOT_CONNECTED) =>
            {
                Ok(0)
            }
            result => result,
        }
    }
}

impl Write for LocalConnection {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let inner = &self.inner.0;
        let mut write_timed_out = self
            .write_timed_out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *write_timed_out {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "local named-pipe connection is unusable after a write timeout",
            ));
        }
        let Some(timeout) = self.write_timeout() else {
            return (&mut &*inner).write(buffer);
        };
        if buffer.is_empty() {
            return Ok(0);
        }
        let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        let result = timed_overlapped_io(inner.as_raw_handle(), timeout, |overlapped| {
            // SAFETY: the buffer is readable for `length` bytes and remains
            // borrowed until timed_overlapped_io completes or drains cancellation.
            unsafe {
                WriteFile(
                    inner.as_raw_handle(),
                    buffer.as_ptr(),
                    length,
                    ptr::null_mut(),
                    overlapped,
                )
            }
        });
        if result.is_ok()
            || result
                .as_ref()
                .is_err_and(|error| error.kind() == std::io::ErrorKind::TimedOut)
        {
            // Direct WriteFile calls bypass interprocess's send wrapper. Keep
            // its linger-on-drop guarantee for successful or partial writes.
            inner.mark_dirty();
        }
        if result
            .as_ref()
            .is_err_and(|error| error.kind() == std::io::ErrorKind::TimedOut)
        {
            // Cancellation can leave an unknown prefix in the peer's stream,
            // so retrying on this connection could duplicate protocol bytes.
            *write_timed_out = true;
        }
        result
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Windows FlushFileBuffers waits for the peer to consume every byte.
        // The facade is unbuffered, so matching Unix's effectively no-op flush
        // avoids turning each newline-delimited request into a deadlock.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn products_have_distinct_user_scoped_endpoints() {
        let quad = LocalEndpoint::for_device(cortex_rs::DeviceKind::QuadCortex);
        let nano = LocalEndpoint::for_device(cortex_rs::DeviceKind::NanoCortex);

        assert_eq!(quad, LocalEndpoint::daemon());
        assert_ne!(quad, nano);
        assert!(quad.to_string().starts_with(r"\\.\pipe\cortex-"));
        assert!(nano.to_string().ends_with("-nano"));
    }

    #[test]
    fn current_process_sid_matches_its_process_token() {
        let sid = current_user_sid().unwrap();

        assert!(!sid.is_empty());
        assert_eq!(sid, process_sid(std::process::id()).unwrap());
    }

    #[test]
    fn missing_endpoint_does_not_wait_for_a_startup_that_was_never_claimed() {
        let endpoint = LocalEndpoint::at(PathBuf::from(format!(
            r"\\.\pipe\cortex-missing-{}",
            std::process::id()
        )));
        let started = Instant::now();

        let Err(error) = LocalConnection::connect(&endpoint) else {
            panic!("a missing endpoint unexpectedly accepted a connection");
        };

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(started.elapsed() < CONNECT_TIMEOUT / 2);
    }

    #[test]
    fn binary_sid_uses_the_sddl_sid_form() {
        let sid = [
            1, 5, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 233, 3, 0, 0,
        ];

        assert_eq!(sid_string(&sid).unwrap(), "S-1-5-21-1-2-3-1001");
    }

    #[test]
    fn an_active_claim_counts_before_the_listener_exists() {
        let endpoint = LocalEndpoint::at(PathBuf::from(format!(
            r"\\.\pipe\cortex-claim-{}",
            std::process::id()
        )));
        let claim = LocalClaim::acquire(&endpoint).unwrap();

        assert!(endpoint.has_active_claim());

        drop(claim);
        assert!(!endpoint.has_active_claim());
    }

    #[test]
    fn byte_stream_pair_is_duplex_and_times_out() {
        let (mut client, mut server) = LocalConnection::pair().unwrap();
        client.write_all(b"request\n").unwrap();
        let mut request = [0_u8; 8];
        server.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"request\n");

        client
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let error = client.read(&mut [0_u8; 1]).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn server_side_reads_wait_for_their_timeout() {
        let (_client, mut server) = LocalConnection::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();

        let error = server.read(&mut [0_u8; 1]).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn a_closed_peer_is_eof_not_a_timeout() {
        let (mut client, server) = LocalConnection::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        drop(server);

        assert_eq!(client.read(&mut [0_u8; 1]).unwrap(), 0);
    }

    #[test]
    fn a_write_timeout_poisons_further_writes() {
        let (mut client, _server) = LocalConnection::pair().unwrap();
        *client.write_timed_out.lock().unwrap() = true;

        let error = client.write(b"retry").unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
}
