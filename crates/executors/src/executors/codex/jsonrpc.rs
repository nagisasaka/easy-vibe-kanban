//! Minimal JSON-RPC helper tailored for the Codex executor.
//!
//! We keep this bespoke layer because the codex-app-server client must handle server-initiated
//! requests as well as client-initiated requests. When a bidirectional client that
//! supports this pattern is available, this module should be straightforward to
//! replace.

use std::{
    collections::HashMap,
    fmt::Debug,
    io,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};

use async_trait::async_trait;
use codex_app_server_protocol::{
    JSONRPCError, JSONRPCMessage, JSONRPCNotification, JSONRPCRequest, JSONRPCResponse, RequestId,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
    sync::{Mutex, mpsc, oneshot},
};
use tokio_util::sync::CancellationToken;

use crate::executors::{ExecutorError, ExecutorExitResult};

#[derive(Debug)]
pub enum PendingResponse {
    Result(Value),
    Error(JSONRPCError),
    Shutdown,
}

#[derive(Debug)]
pub enum JsonRpcControlFlow {
    Continue,
    Exit(ExecutorExitResult),
}

#[derive(Clone)]
pub struct ExitSignalSender {
    inner: Arc<Mutex<Option<oneshot::Sender<ExecutorExitResult>>>>,
}

impl ExitSignalSender {
    pub fn new(sender: oneshot::Sender<ExecutorExitResult>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(sender))),
        }
    }

    pub async fn send_exit_signal(&self, result: ExecutorExitResult) {
        if let Some(sender) = self.inner.lock().await.take() {
            let _ = sender.send(result);
        }
    }
}

#[derive(Clone)]
pub struct JsonRpcPeer {
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<PendingResponse>>>>,
    id_counter: Arc<AtomicI64>,
    exit_requests: mpsc::UnboundedSender<ExecutorExitResult>,
}

impl JsonRpcPeer {
    pub fn spawn(
        stdin: ChildStdin,
        stdout: ChildStdout,
        callbacks: Arc<dyn JsonRpcCallbacks>,
        exit_tx: ExitSignalSender,
        cancel: CancellationToken,
    ) -> Self {
        let (exit_requests, mut exit_requests_rx) = mpsc::unbounded_channel();
        let peer = Self {
            stdin: Arc::new(Mutex::new(stdin)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            id_counter: Arc::new(AtomicI64::new(1)),
            exit_requests,
        };

        let reader_peer = peer.clone();
        let callbacks = callbacks.clone();

        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut buffer = Vec::new();
            let mut deferred_success = false;

            let exit_result = loop {
                // An idle completion must not shut down outstanding control RPCs.
                // All response errors are observed before a successful close.
                if deferred_success && reader_peer.pending.lock().await.is_empty() {
                    break ExecutorExitResult::Success;
                }
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::debug!("Codex executor cancelled");
                        break ExecutorExitResult::Failure;
                    }
                    Some(result) = exit_requests_rx.recv() => {
                        match result {
                            ExecutorExitResult::Failure => break ExecutorExitResult::Failure,
                            ExecutorExitResult::Success => { deferred_success = true; continue; }
                        }
                    }
                    // read_until is cancellation-safe when an exit request wins
                    // select; preserve a partially received frame across iterations.
                    read_result = reader.read_until(b'\n', &mut buffer) => {
                        match read_result {
                            Ok(0) => {
                                tracing::warn!("Codex app-server stdout closed before completion");
                                break ExecutorExitResult::Failure;
                            }
                            Ok(_) => {
                                let bytes = std::mem::take(&mut buffer);
                                let raw = String::from_utf8_lossy(&bytes);
                                let line = raw.trim_end_matches(['\n', '\r']);
                                if line.is_empty() {
                                    continue;
                                }

                                match serde_json::from_str::<JSONRPCMessage>(line) {
                                    Ok(JSONRPCMessage::Response(response)) => {
                                        let request_id = response.id.clone();
                                        let result = response.result.clone();
                                        if let Err(err) = callbacks
                                            .on_response(&reader_peer, line, &response)
                                            .await
                                        {
                                            tracing::warn!("Codex response callback failed: {err}");
                                            break ExecutorExitResult::Failure;
                                        }
                                        reader_peer
                                            .resolve(request_id, PendingResponse::Result(result))
                                            .await;
                                    }
                                    Ok(JSONRPCMessage::Error(error)) => {
                                        let request_id = error.id.clone();
                                        let callback_result = callbacks
                                            .on_error(&reader_peer, line, &error)
                                            .await;
                                        reader_peer
                                            .resolve(request_id, PendingResponse::Error(error))
                                            .await;
                                        if let Err(err) = callback_result {
                                            tracing::warn!("Codex error callback failed: {err}");
                                            break ExecutorExitResult::Failure;
                                        }
                                        if deferred_success { break ExecutorExitResult::Failure; }
                                    }
                                    Ok(JSONRPCMessage::Request(request)) => {
                                        if let Err(err) = callbacks
                                            .on_request(&reader_peer, line, request)
                                            .await
                                        {
                                            tracing::warn!("Codex request callback failed: {err}");
                                            break ExecutorExitResult::Failure;
                                        }
                                    }
                                    Ok(JSONRPCMessage::Notification(notification)) => {
                                        match callbacks
                                            .on_notification(&reader_peer, line, notification)
                                            .await
                                        {
                                            Ok(JsonRpcControlFlow::Exit(ExecutorExitResult::Failure)) => break ExecutorExitResult::Failure,
                                            Ok(JsonRpcControlFlow::Exit(ExecutorExitResult::Success)) => deferred_success = true,
                                            Ok(JsonRpcControlFlow::Continue) => {}
                                            Err(err) => {
                                                tracing::warn!("Codex notification callback failed: {err}");
                                                break ExecutorExitResult::Failure;
                                            }
                                        }
                                    }
                                    Err(_) => {
                                        if let Err(err) = callbacks.on_non_json(line).await {
                                            tracing::warn!("Codex non-JSON callback failed: {err}");
                                            break ExecutorExitResult::Failure;
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::warn!("Error reading Codex output: {err}");
                                break ExecutorExitResult::Failure;
                            }
                        }
                    }
                }
            };

            let _ = reader_peer.shutdown().await;
            // Release background approval/plan tasks on every terminal path.
            cancel.cancel();
            exit_tx.send_exit_signal(exit_result).await;
        });

        peer
    }

    pub fn next_request_id(&self) -> RequestId {
        RequestId::Integer(self.id_counter.fetch_add(1, Ordering::Relaxed))
    }

    pub fn request_exit(&self, result: ExecutorExitResult) {
        let _ = self.exit_requests.send(result);
    }

    pub async fn register(&self, request_id: RequestId) -> PendingReceiver {
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(request_id, sender);
        receiver
    }

    pub async fn resolve(&self, request_id: RequestId, response: PendingResponse) {
        if let Some(sender) = self.pending.lock().await.remove(&request_id) {
            let _ = sender.send(response);
        }
    }

    pub async fn shutdown(&self) -> Result<(), ExecutorError> {
        let mut pending = self.pending.lock().await;
        for (_, sender) in pending.drain() {
            let _ = sender.send(PendingResponse::Shutdown);
        }
        Ok(())
    }

    pub async fn send<T>(&self, message: &T) -> Result<(), ExecutorError>
    where
        T: Serialize + Sync,
    {
        self.send_with_raw(message).await.map(|_| ())
    }

    pub async fn send_with_raw<T>(&self, message: &T) -> Result<Vec<u8>, ExecutorError>
    where
        T: Serialize + Sync,
    {
        let mut raw = serde_json::to_vec(message)
            .map_err(|err| ExecutorError::Io(io::Error::other(err.to_string())))?;
        raw.push(b'\n');
        self.send_raw(&raw).await?;
        Ok(raw)
    }

    pub async fn request<R, T>(
        &self,
        request_id: RequestId,
        message: &T,
        label: &str,
        cancel: CancellationToken,
    ) -> Result<R, ExecutorError>
    where
        R: DeserializeOwned + Debug,
        T: Serialize + Sync,
    {
        self.request_with_raw(request_id, message, label, cancel)
            .await
            .map(|(response, _)| response)
    }

    pub async fn request_with_raw<R, T>(
        &self,
        request_id: RequestId,
        message: &T,
        label: &str,
        cancel: CancellationToken,
    ) -> Result<(R, Vec<u8>), ExecutorError>
    where
        R: DeserializeOwned + Debug,
        T: Serialize + Sync,
    {
        let mut raw = serde_json::to_vec(message)
            .map_err(|err| ExecutorError::Io(io::Error::other(err.to_string())))?;
        raw.push(b'\n');
        let receiver = self.register(request_id.clone()).await;
        if let Err(error) = self.send_raw(&raw).await {
            self.pending.lock().await.remove(&request_id);
            self.request_exit(ExecutorExitResult::Failure);
            return Err(error);
        }
        // This bounds RPC acknowledgements, never model execution or approval waits.
        let response = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            await_response(receiver, label, cancel),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                self.request_exit(ExecutorExitResult::Failure);
                Err(ExecutorError::Io(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{label} acknowledgement timed out"),
                )))
            }
        };
        self.pending.lock().await.remove(&request_id);
        let response = response?;
        Ok((response, raw))
    }

    async fn send_raw(&self, payload: &[u8]) -> Result<(), ExecutorError> {
        let mut guard = self.stdin.lock().await;
        guard.write_all(payload).await.map_err(ExecutorError::Io)?;
        guard.flush().await.map_err(ExecutorError::Io)?;
        Ok(())
    }
}

pub type PendingReceiver = oneshot::Receiver<PendingResponse>;

pub async fn await_response<R>(
    receiver: PendingReceiver,
    label: &str,
    cancel: CancellationToken,
) -> Result<R, ExecutorError>
where
    R: DeserializeOwned + Debug,
{
    let response = tokio::select! {
        // A response already read from the wire remains authoritative even if
        // the run completed and cancelled background work before this task woke.
        biased;
        result = receiver => result,
        _ = cancel.cancelled() => {
            return Err(ExecutorError::Io(io::Error::other(format!(
                "{label} request cancelled",
            ))));
        }
    };

    match response {
        Ok(PendingResponse::Result(value)) => serde_json::from_value(value).map_err(|err| {
            ExecutorError::Io(io::Error::other(format!(
                "failed to decode {label} response: {err}",
            )))
        }),
        Ok(PendingResponse::Error(error)) => Err(ExecutorError::Io(io::Error::other(format!(
            "{label} request failed: {}",
            error.error.message
        )))),
        Ok(PendingResponse::Shutdown) => Err(ExecutorError::Io(io::Error::other(format!(
            "server was shutdown while waiting for {label} response",
        )))),
        Err(_) => Err(ExecutorError::Io(io::Error::other(format!(
            "{label} request was dropped",
        )))),
    }
}

#[async_trait]
pub trait JsonRpcCallbacks: Send + Sync {
    async fn on_request(
        &self,
        peer: &JsonRpcPeer,
        raw: &str,
        request: JSONRPCRequest,
    ) -> Result<(), ExecutorError>;

    async fn on_response(
        &self,
        peer: &JsonRpcPeer,
        raw: &str,
        response: &JSONRPCResponse,
    ) -> Result<(), ExecutorError>;

    async fn on_error(
        &self,
        peer: &JsonRpcPeer,
        raw: &str,
        error: &JSONRPCError,
    ) -> Result<(), ExecutorError>;

    async fn on_notification(
        &self,
        peer: &JsonRpcPeer,
        raw: &str,
        notification: JSONRPCNotification,
    ) -> Result<JsonRpcControlFlow, ExecutorError>;

    async fn on_non_json(&self, _raw: &str) -> Result<(), ExecutorError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn received_response_wins_over_later_shutdown_cancellation() {
        let (tx, rx) = oneshot::channel();
        tx.send(PendingResponse::Result(serde_json::json!(42)))
            .unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert_eq!(await_response::<u32>(rx, "test", cancel).await.unwrap(), 42);
    }

    #[tokio::test]
    async fn unacknowledged_request_is_cancelled_not_succeeded() {
        let (_tx, rx) = oneshot::channel();
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(await_response::<u32>(rx, "test", cancel).await.is_err());
    }
}
