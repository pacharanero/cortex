// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Bounded newline-delimited stdio transport for `rmcp`.
//!
//! `rmcp` 3.1.0 still uses an unbounded `read_until` on its stdio read path.
//! This adapter caps each line and aggregate in-flight work until the response
//! has actually been written.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex};

use rmcp::ErrorData;
use rmcp::model::{ErrorCode, GetExtensions, JsonRpcMessage, RequestId};
use rmcp::service::{RoleServer, RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
const MAX_IN_FLIGHT_REQUESTS: usize = 8;

#[derive(Clone)]
struct RequestGuard {
    _inner: Arc<RequestGuardInner>,
}

struct RequestGuardInner {
    registry: Arc<InFlightRegistry>,
    id: RequestId,
}

impl Drop for RequestGuardInner {
    fn drop(&mut self) {
        self.registry.handler_finished(&self.id);
    }
}

#[derive(Clone)]
struct NotificationGuard {
    _permit: Arc<OwnedSemaphorePermit>,
}

struct InFlightRequest {
    _permit: OwnedSemaphorePermit,
    handler_finished: bool,
    cancelled: bool,
    response_started: bool,
}

struct InFlightRegistry {
    slots: Arc<Semaphore>,
    requests: Mutex<HashMap<RequestId, InFlightRequest>>,
}

enum StartError {
    Closed,
    Duplicate,
    Full,
}

impl InFlightRegistry {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            slots: Arc::new(Semaphore::new(MAX_IN_FLIGHT_REQUESTS)),
            requests: Mutex::new(HashMap::new()),
        })
    }

    fn start(self: &Arc<Self>, id: RequestId) -> Result<RequestGuard, StartError> {
        let permit = match self.slots.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(TryAcquireError::Closed) => return Err(StartError::Closed),
            Err(TryAcquireError::NoPermits) => return Err(StartError::Full),
        };
        let mut requests = self
            .requests
            .lock()
            .unwrap_or_else(|lock| lock.into_inner());
        if requests.contains_key(&id) {
            return Err(StartError::Duplicate);
        }
        requests.insert(
            id.clone(),
            InFlightRequest {
                _permit: permit,
                handler_finished: false,
                cancelled: false,
                response_started: false,
            },
        );
        Ok(RequestGuard {
            _inner: Arc::new(RequestGuardInner {
                registry: self.clone(),
                id,
            }),
        })
    }

    fn handler_finished(&self, id: &RequestId) {
        let mut requests = self
            .requests
            .lock()
            .unwrap_or_else(|lock| lock.into_inner());
        if let Some(request) = requests.get_mut(id) {
            request.handler_finished = true;
            if request.cancelled && !request.response_started {
                requests.remove(id);
            }
        }
    }

    fn cancel(&self, id: &RequestId) {
        let mut requests = self
            .requests
            .lock()
            .unwrap_or_else(|lock| lock.into_inner());
        if let Some(request) = requests.get_mut(id) {
            request.cancelled = true;
            if request.handler_finished && !request.response_started {
                requests.remove(id);
            }
        }
    }

    fn response_started(&self, id: &RequestId) {
        if let Some(request) = self
            .requests
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .get_mut(id)
        {
            request.response_started = true;
        }
    }

    fn response_finished(&self, id: &RequestId) {
        self.requests
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .remove(id);
    }
}

/// Newline-delimited MCP stdio with finite input and request-work bounds.
pub struct BoundedStdioTransport<R, W> {
    reader: BufReader<R>,
    line: Vec<u8>,
    writer: Arc<tokio::sync::Mutex<Option<W>>>,
    in_flight: Arc<InFlightRegistry>,
    notification_slots: Arc<Semaphore>,
}

impl BoundedStdioTransport<tokio::io::Stdin, tokio::io::Stdout> {
    /// Bind the transport to process stdin and stdout.
    pub fn stdio() -> Self {
        Self::new(tokio::io::stdin(), tokio::io::stdout())
    }
}

impl<R, W> BoundedStdioTransport<R, W>
where
    R: AsyncRead + Unpin,
{
    fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            line: Vec::new(),
            writer: Arc::new(tokio::sync::Mutex::new(Some(writer))),
            in_flight: InFlightRegistry::new(),
            notification_slots: Arc::new(Semaphore::new(MAX_IN_FLIGHT_REQUESTS)),
        }
    }
}

impl<R, W> Transport<RoleServer> for BoundedStdioTransport<R, W>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let writer = self.writer.clone();
        let in_flight = self.in_flight.clone();
        let response_id = match &item {
            JsonRpcMessage::Response(response) => Some(response.id.clone()),
            JsonRpcMessage::Error(error) => error.id.clone(),
            JsonRpcMessage::Request(_) | JsonRpcMessage::Notification(_) => None,
        };
        if let Some(id) = response_id.as_ref() {
            in_flight.response_started(id);
        }
        async move {
            let result = write_json_line(&writer, item).await;
            if let Some(id) = response_id.as_ref() {
                in_flight.response_finished(id);
            }
            result
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        loop {
            match read_bounded_line(&mut self.reader, &mut self.line).await {
                Ok(false) => return None,
                Ok(true) => {}
                Err(error) => {
                    eprintln!("cortex-mcp: {error}");
                    return None;
                }
            }
            let value = {
                let line = self.line.strip_suffix(b"\r").unwrap_or(&self.line);
                serde_json::from_slice::<Value>(line)
            };
            self.line.clear();
            let value = match value {
                Ok(value) => value,
                Err(error) => {
                    let message = TxJsonRpcMessage::<RoleServer>::error(
                        ErrorData::parse_error(format!("Parse error: {error}"), None),
                        None,
                    );
                    if write_json_line(&self.writer, message).await.is_err() {
                        return None;
                    }
                    continue;
                }
            };
            let request_id = request_id(&value);
            let cancelled = cancelled_request_id(&value);
            let mut message = match serde_json::from_value::<RxJsonRpcMessage<RoleServer>>(value) {
                Ok(message) => message,
                Err(_) => {
                    let message = TxJsonRpcMessage::<RoleServer>::error(
                        ErrorData::invalid_request("Invalid request", None),
                        request_id,
                    );
                    if write_json_line(&self.writer, message).await.is_err() {
                        return None;
                    }
                    continue;
                }
            };
            if let JsonRpcMessage::Request(request) = &mut message {
                match self.in_flight.start(request.id.clone()) {
                    Ok(guard) => {
                        request.request.extensions_mut().insert(guard);
                    }
                    Err(StartError::Duplicate) => {
                        let message = TxJsonRpcMessage::<RoleServer>::error(
                            ErrorData::invalid_request("Duplicate in-flight request id", None),
                            Some(request.id.clone()),
                        );
                        if write_json_line(&self.writer, message).await.is_err() {
                            return None;
                        }
                        continue;
                    }
                    Err(StartError::Full) => {
                        let message = TxJsonRpcMessage::<RoleServer>::error(
                            ErrorData::new(ErrorCode(-32000), "Too many in-flight requests", None),
                            Some(request.id.clone()),
                        );
                        if write_json_line(&self.writer, message).await.is_err() {
                            return None;
                        }
                        continue;
                    }
                    Err(StartError::Closed) => return None,
                }
            } else if let JsonRpcMessage::Notification(notification) = &mut message {
                let Ok(permit) = self.notification_slots.clone().try_acquire_owned() else {
                    continue;
                };
                notification
                    .notification
                    .extensions_mut()
                    .insert(NotificationGuard {
                        _permit: Arc::new(permit),
                    });
                if let Some(id) = cancelled.as_ref() {
                    self.in_flight.cancel(id);
                }
            }
            return Some(message);
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.writer.lock().await.take();
        Ok(())
    }
}

fn request_id(value: &Value) -> Option<RequestId> {
    id_value(value.get("id")?)
}

fn cancelled_request_id(value: &Value) -> Option<RequestId> {
    if value.get("method").and_then(Value::as_str) != Some("notifications/cancelled") {
        return None;
    }
    id_value(value.get("params")?.get("requestId")?)
}

fn id_value(value: &Value) -> Option<RequestId> {
    match value {
        Value::String(id) => Some(RequestId::String(Arc::from(id.as_str()))),
        Value::Number(id) => id.as_i64().map(RequestId::Number),
        _ => None,
    }
}

async fn read_bounded_line<R>(reader: &mut R, line: &mut Vec<u8>) -> io::Result<bool>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let (consumed, complete) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                if line.is_empty() {
                    return Ok(false);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "input ended before the newline message delimiter",
                ));
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let content_len = newline.unwrap_or(available.len());
            if line.len().saturating_add(content_len) > MAX_MESSAGE_SIZE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("input line exceeds maximum accepted size ({MAX_MESSAGE_SIZE} bytes)"),
                ));
            }
            line.extend_from_slice(&available[..content_len]);
            (
                content_len + usize::from(newline.is_some()),
                newline.is_some(),
            )
        };
        reader.consume(consumed);
        if complete {
            return Ok(true);
        }
    }
}

async fn write_json_line<W>(
    writer: &Arc<tokio::sync::Mutex<Option<W>>>,
    message: TxJsonRpcMessage<RoleServer>,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut writer = writer.lock().await;
    let writer = writer
        .as_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "transport is closed"))?;
    let bytes = serde_json::to_vec(&message).map_err(io::Error::other)?;
    writer.write_all(&bytes).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_reader_accepts_a_complete_line() {
        let mut reader = BufReader::new(&b"{}\n"[..]);
        let mut line = Vec::new();
        assert!(read_bounded_line(&mut reader, &mut line).await.unwrap());
        assert_eq!(line, b"{}");
    }

    #[tokio::test]
    async fn bounded_reader_rejects_unterminated_input() {
        let mut reader = BufReader::new(&b"{}"[..]);
        let mut line = Vec::new();
        let error = read_bounded_line(&mut reader, &mut line).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
}
