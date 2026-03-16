//! Channel trait implementation for channel-relay webhook callbacks.
//!
//! `RelayChannel` receives events from channel-relay via HTTP POST callbacks
//! (pushed through an mpsc channel by the webhook handler), converts them
//! to `IncomingMessage`s, and sends responses via the relay's provider-specific
//! proxy API (Slack).

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::channels::relay::client::{ChannelEvent, RelayClient};
use crate::channels::{Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate};
use crate::error::ChannelError;

/// Default channel name for the Slack relay integration.
pub const DEFAULT_RELAY_NAME: &str = "slack-relay";

/// The messaging provider backing a relay channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayProvider {
    Slack,
}

impl RelayProvider {
    /// Provider string used in proxy API routes and metadata.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Slack => "slack",
        }
    }

    /// The default channel name for this provider.
    pub fn channel_name(&self) -> &'static str {
        match self {
            Self::Slack => DEFAULT_RELAY_NAME,
        }
    }
}

/// Channel implementation that receives events from channel-relay via webhook callbacks.
pub struct RelayChannel {
    client: RelayClient,
    provider: RelayProvider,
    team_id: String,
    instance_id: String,
    /// Sender side of the event channel — shared with the webhook handler.
    event_tx: mpsc::Sender<ChannelEvent>,
    /// Receiver side — taken once by `start()`.
    event_rx: tokio::sync::Mutex<Option<mpsc::Receiver<ChannelEvent>>>,
}

impl RelayChannel {
    /// Create a new relay channel for Slack (default provider).
    pub fn new(
        client: RelayClient,
        team_id: String,
        instance_id: String,
        event_tx: mpsc::Sender<ChannelEvent>,
        event_rx: mpsc::Receiver<ChannelEvent>,
    ) -> Self {
        Self::new_with_provider(
            client,
            RelayProvider::Slack,
            team_id,
            instance_id,
            event_tx,
            event_rx,
        )
    }

    /// Create a new relay channel with a specific provider.
    pub fn new_with_provider(
        client: RelayClient,
        provider: RelayProvider,
        team_id: String,
        instance_id: String,
        event_tx: mpsc::Sender<ChannelEvent>,
        event_rx: mpsc::Receiver<ChannelEvent>,
    ) -> Self {
        Self {
            client,
            provider,
            team_id,
            instance_id,
            event_tx,
            event_rx: tokio::sync::Mutex::new(Some(event_rx)),
        }
    }

    /// Get a clone of the event sender for wiring into the webhook endpoint.
    pub fn event_sender(&self) -> mpsc::Sender<ChannelEvent> {
        self.event_tx.clone()
    }

    /// Build a provider-appropriate proxy body for sending a message.
    fn build_send_body(
        &self,
        channel_id: &str,
        text: &str,
        thread_id: Option<&str>,
    ) -> (String, serde_json::Value) {
        match self.provider {
            RelayProvider::Slack => {
                let mut body = serde_json::json!({
                    "channel": channel_id,
                    "text": text,
                });
                if let Some(tid) = thread_id {
                    body["thread_ts"] = serde_json::Value::String(tid.to_string());
                }
                ("chat.postMessage".to_string(), body)
            }
        }
    }

    /// Send a message via the provider proxy.
    async fn proxy_send(
        &self,
        team_id: &str,
        method: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, crate::channels::relay::client::RelayError> {
        self.client
            .proxy_provider(
                self.provider.as_str(),
                team_id,
                method,
                body,
                Some(&self.instance_id),
            )
            .await
    }
}

#[async_trait]
impl Channel for RelayChannel {
    fn name(&self) -> &str {
        self.provider.channel_name()
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let channel_name = self.name().to_string();

        // Take the receiver (can only start once)
        let mut event_rx =
            self.event_rx
                .lock()
                .await
                .take()
                .ok_or_else(|| ChannelError::StartupFailed {
                    name: channel_name.clone(),
                    reason: "RelayChannel already started".to_string(),
                })?;

        let (tx, rx) = mpsc::channel(64);
        let provider_str = self.provider.as_str().to_string();
        let relay_name = channel_name.clone();

        // Spawn a task that reads events from the webhook handler and converts to IncomingMessage
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                // Validate required fields
                if event.sender_id.is_empty()
                    || event.channel_id.is_empty()
                    || event.provider_scope.is_empty()
                {
                    tracing::debug!(
                        event_type = %event.event_type,
                        sender_id = %event.sender_id,
                        channel_id = %event.channel_id,
                        "Relay: skipping event with missing required fields"
                    );
                    continue;
                }

                // Skip non-message events
                if !event.is_message() {
                    tracing::debug!(
                        event_type = %event.event_type,
                        "Relay: skipping non-message event"
                    );
                    continue;
                }

                tracing::info!(
                    event_type = %event.event_type,
                    sender = %event.sender_id,
                    channel = %event.channel_id,
                    provider = %provider_str,
                    "Relay: received message from {}", provider_str
                );

                let msg = IncomingMessage::new(&relay_name, &event.sender_id, event.text())
                    .with_user_name(event.display_name())
                    .with_metadata(serde_json::json!({
                        "team_id": event.team_id(),
                        "channel_id": event.channel_id,
                        "sender_id": event.sender_id,
                        "sender_name": event.display_name(),
                        "event_type": event.event_type,
                        "thread_id": event.thread_id,
                        "provider": event.provider,
                    }));

                let msg = if let Some(ref thread_id) = event.thread_id {
                    msg.with_thread(thread_id)
                } else {
                    msg.with_thread(&event.channel_id)
                };

                if tx.send(msg).await.is_err() {
                    tracing::info!("Relay channel receiver dropped, stopping");
                    return;
                }
            }

            tracing::info!("Relay event channel closed");
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let channel_name = self.name().to_string();
        let metadata = &msg.metadata;
        let team_id = metadata
            .get("team_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.team_id);
        let channel_id = metadata
            .get("channel_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ChannelError::SendFailed {
                name: channel_name.clone(),
                reason: "Missing channel_id in message metadata".to_string(),
            })?;

        // Determine thread_id from response or metadata
        let thread_id = response
            .thread_id
            .as_deref()
            .or_else(|| metadata.get("thread_id").and_then(|v| v.as_str()));

        let (method, body) = self.build_send_body(channel_id, &response.content, thread_id);

        self.proxy_send(team_id, &method, body)
            .await
            .map_err(|e| ChannelError::SendFailed {
                name: channel_name,
                reason: e.to_string(),
            })?;

        Ok(())
    }

    /// Status updates are not forwarded to messaging providers to avoid noise.
    async fn send_status(
        &self,
        _status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn broadcast(
        &self,
        target: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let channel_name = self.name().to_string();

        // Determine thread_id from response or metadata
        let thread_id = response
            .thread_id
            .as_deref()
            .or_else(|| response.metadata.get("thread_ts").and_then(|v| v.as_str()));

        let (method, body) = self.build_send_body(target, &response.content, thread_id);

        self.proxy_send(&self.team_id, &method, body)
            .await
            .map_err(|e| ChannelError::SendFailed {
                name: channel_name,
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        self.client
            .list_connections(&self.instance_id)
            .await
            .map_err(|_| ChannelError::HealthCheckFailed {
                name: self.name().to_string(),
            })?;
        Ok(())
    }

    fn conversation_context(&self, metadata: &serde_json::Value) -> HashMap<String, String> {
        let mut ctx = HashMap::new();

        if let Some(sender) = metadata.get("sender_name").and_then(|v| v.as_str()) {
            ctx.insert("sender".to_string(), sender.to_string());
        }
        if let Some(sender_id) = metadata.get("sender_id").and_then(|v| v.as_str()) {
            ctx.insert("sender_uuid".to_string(), sender_id.to_string());
        }
        if let Some(channel_id) = metadata.get("channel_id").and_then(|v| v.as_str()) {
            ctx.insert("group".to_string(), channel_id.to_string());
        }
        ctx.insert("platform".to_string(), self.provider.as_str().to_string());

        ctx
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        // Nothing to clean up — the event channel will close naturally
        // when the sender is dropped
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> RelayClient {
        RelayClient::new(
            "http://localhost:3001".into(),
            secrecy::SecretString::from("key".to_string()),
            30,
        )
        .expect("client")
    }

    fn make_channel() -> RelayChannel {
        let (tx, rx) = mpsc::channel(64);
        RelayChannel::new(test_client(), "T123".into(), "inst1".into(), tx, rx)
    }

    #[test]
    fn relay_channel_name() {
        let channel = make_channel();
        assert_eq!(channel.name(), DEFAULT_RELAY_NAME);
    }

    #[test]
    fn conversation_context_extracts_metadata() {
        let channel = make_channel();

        let metadata = serde_json::json!({
            "sender_name": "bob",
            "sender_id": "U123",
            "channel_id": "C456",
        });
        let ctx = channel.conversation_context(&metadata);
        assert_eq!(ctx.get("sender"), Some(&"bob".to_string()));
        assert_eq!(ctx.get("sender_uuid"), Some(&"U123".to_string()));
        assert_eq!(ctx.get("platform"), Some(&"slack".to_string()));
    }

    #[test]
    fn metadata_shape_includes_event_type_and_sender_name() {
        let metadata = serde_json::json!({
            "team_id": "T123",
            "channel_id": "C456",
            "sender_id": "U789",
            "sender_name": "alice",
            "event_type": "direct_message",
            "thread_id": null,
            "provider": "slack",
        });
        assert_eq!(
            metadata.get("event_type").and_then(|v| v.as_str()),
            Some("direct_message")
        );
        assert_eq!(
            metadata.get("sender_name").and_then(|v| v.as_str()),
            Some("alice")
        );
    }

    #[test]
    fn build_send_body_slack() {
        let channel = make_channel();
        let (method, body) = channel.build_send_body("C456", "hello", Some("1234567.890"));
        assert_eq!(method, "chat.postMessage");
        assert_eq!(body["channel"], "C456");
        assert_eq!(body["text"], "hello");
        assert_eq!(body["thread_ts"], "1234567.890");
    }

    #[tokio::test]
    async fn start_processes_events() {
        let (tx, rx) = mpsc::channel(64);
        let channel =
            RelayChannel::new(test_client(), "T123".into(), "inst1".into(), tx.clone(), rx);

        let mut stream = channel.start().await.unwrap();

        // Send an event
        tx.send(ChannelEvent {
            id: "1".into(),
            event_type: "message".into(),
            provider: "slack".into(),
            provider_scope: "T123".into(),
            channel_id: "C456".into(),
            sender_id: "U789".into(),
            sender_name: Some("alice".into()),
            content: Some("hello".into()),
            thread_id: None,
            raw: serde_json::Value::Null,
            timestamp: None,
        })
        .await
        .unwrap();

        use futures::StreamExt;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(msg.content, "hello");
        assert_eq!(msg.user_id, "U789");
    }

    #[tokio::test]
    async fn start_skips_non_message_events() {
        let (tx, rx) = mpsc::channel(64);
        let channel =
            RelayChannel::new(test_client(), "T123".into(), "inst1".into(), tx.clone(), rx);

        let mut stream = channel.start().await.unwrap();

        // Send a non-message event (should be skipped)
        tx.send(ChannelEvent {
            id: "1".into(),
            event_type: "reaction".into(),
            provider: "slack".into(),
            provider_scope: "T123".into(),
            channel_id: "C456".into(),
            sender_id: "U789".into(),
            sender_name: None,
            content: None,
            thread_id: None,
            raw: serde_json::Value::Null,
            timestamp: None,
        })
        .await
        .unwrap();

        // Send a real message
        tx.send(ChannelEvent {
            id: "2".into(),
            event_type: "message".into(),
            provider: "slack".into(),
            provider_scope: "T123".into(),
            channel_id: "C456".into(),
            sender_id: "U789".into(),
            sender_name: None,
            content: Some("real message".into()),
            thread_id: None,
            raw: serde_json::Value::Null,
            timestamp: None,
        })
        .await
        .unwrap();

        use futures::StreamExt;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(msg.content, "real message");
    }
}
