//! # aria-mesh
//!
//! L4 Mesh Transport layer for ARIA-X.
//!
//! Provides [`ZenohRouter`] — a Zenoh-based pub/sub wrapper implementing
//! the ARIA-X topic hierarchy:
//!
//! | Pattern | Direction | Purpose |
//! |---------|-----------|---------|
//! | `aria/gateway/{channel}/inbound` | Publish | Ingress from gateway channels |
//! | `aria/skill/{node}/call/{skill}` | Publish | Dispatch skill invocations |
//! | `aria/skill/+/result` | Subscribe | Collect skill execution results |

use std::sync::Arc;

use aria_core::{AgentRequest, AgentResponse, AriaError};
use serde::Serialize;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors originating from the mesh transport layer.
#[derive(Debug)]
pub enum MeshError {
    /// Failed to open or interact with the Zenoh session.
    ZenohError(String),
    /// Serialization or deserialization of a message failed.
    SerializationError(String),
    /// A receive operation timed out.
    Timeout(String),
    /// The session has already been closed.
    SessionClosed,
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshError::ZenohError(msg) => write!(f, "zenoh error: {}", msg),
            MeshError::SerializationError(msg) => write!(f, "serialization error: {}", msg),
            MeshError::Timeout(msg) => write!(f, "timeout: {}", msg),
            MeshError::SessionClosed => write!(f, "session closed"),
        }
    }
}

impl std::error::Error for MeshError {}

impl From<AriaError> for MeshError {
    fn from(err: AriaError) -> Self {
        MeshError::SerializationError(format!("{}", err))
    }
}

// ---------------------------------------------------------------------------
// ZenohRouter
// ---------------------------------------------------------------------------

/// Configuration for creating a [`ZenohRouter`].
#[derive(Debug, Clone)]
pub struct MeshConfig {
    /// Zenoh connect endpoints (e.g. `tcp/localhost:7447`).
    /// If empty, an in-memory peer session is created (useful for testing).
    pub connect_endpoints: Vec<String>,
    /// Whether to operate in peer mode (true) or client mode (false).
    pub peer_mode: bool,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            connect_endpoints: Vec::new(),
            peer_mode: true,
        }
    }
}

/// Zenoh-based pub/sub router for the ARIA-X L4 mesh transport layer.
///
/// Wraps a [`zenoh::Session`] and provides topic-aware publish/subscribe
/// methods following the ARIA-X key hierarchy.
pub struct ZenohRouter {
    session: Arc<zenoh::Session>,
}

impl ZenohRouter {
    /// Open a new Zenoh session with the given configuration.
    pub async fn new(config: MeshConfig) -> Result<Self, MeshError> {
        let mut zenoh_config = zenoh::Config::default();

        if config.peer_mode {
            zenoh_config
                .insert_json5("mode", r#""peer""#)
                .map_err(|e| MeshError::ZenohError(format!("config mode: {}", e)))?;
        } else {
            zenoh_config
                .insert_json5("mode", r#""client""#)
                .map_err(|e| MeshError::ZenohError(format!("config mode: {}", e)))?;
        }

        if !config.connect_endpoints.is_empty() {
            let endpoints_json = serde_json::to_string(&config.connect_endpoints)
                .map_err(|e| MeshError::SerializationError(format!("{}", e)))?;
            zenoh_config
                .insert_json5("connect/endpoints", &endpoints_json)
                .map_err(|e| MeshError::ZenohError(format!("config endpoints: {}", e)))?;
        }

        let session = zenoh::open(zenoh_config)
            .await
            .map_err(|e| MeshError::ZenohError(format!("open: {}", e)))?;

        Ok(Self {
            session: Arc::new(session),
        })
    }

    /// Create a router from an existing Zenoh session (useful for testing).
    pub fn from_session(session: Arc<zenoh::Session>) -> Self {
        Self { session }
    }

    /// Publish an [`AgentRequest`] to the gateway inbound topic.
    ///
    /// Topic: `aria/gateway/{channel}/inbound`
    pub async fn publish_inbound(
        &self,
        channel: &str,
        request: &AgentRequest,
    ) -> Result<(), MeshError> {
        let topic = format!("aria/gateway/{}/inbound", channel);
        self.publish(&topic, request).await
    }

    /// Publish payload to a skill invocation topic.
    ///
    /// Topic: `aria/skill/{node}/call/{skill}`
    pub async fn publish_skill_call<T: Serialize>(
        &self,
        node: &str,
        skill: &str,
        payload: &T,
    ) -> Result<(), MeshError> {
        let topic = format!("aria/skill/{}/call/{}", node, skill);
        self.publish(&topic, payload).await
    }

    /// Subscribe to all skill results using the Zenoh wildcard topic.
    ///
    /// Topic: `aria/skill/*/result`
    ///
    /// Returns an `mpsc::Receiver` that yields deserialized [`AgentResponse`]
    /// values as they arrive.
    pub async fn subscribe_results(
        &self,
        buffer_size: usize,
    ) -> Result<mpsc::Receiver<Result<AgentResponse, MeshError>>, MeshError> {
        let (tx, rx) = mpsc::channel(buffer_size);
        let subscriber = self
            .session
            .declare_subscriber("aria/skill/*/result")
            .await
            .map_err(|e| MeshError::ZenohError(format!("subscribe: {}", e)))?;

        tokio::spawn(async move {
            while let Ok(sample) = subscriber.recv_async().await {
                let payload_bytes: Vec<u8> = sample.payload().to_bytes().to_vec();
                let result: Result<AgentResponse, MeshError> =
                    serde_json::from_slice(&payload_bytes)
                        .map_err(|e| MeshError::SerializationError(format!("{}", e)));
                if tx.send(result).await.is_err() {
                    // Receiver dropped — stop the loop.
                    break;
                }
            }
        });

        Ok(rx)
    }

    /// Gracefully close the Zenoh session.
    pub async fn close(&self) -> Result<(), MeshError> {
        self.session
            .close()
            .await
            .map_err(|e| MeshError::ZenohError(format!("close: {}", e)))
    }

    /// Get a reference to the underlying Zenoh session.
    pub fn session(&self) -> &Arc<zenoh::Session> {
        &self.session
    }

    // -- Internal helpers --------------------------------------------------

    /// Serialize `value` as JSON and publish to `topic`.
    async fn publish<T: Serialize>(&self, topic: &str, value: &T) -> Result<(), MeshError> {
        let bytes = serde_json::to_vec(value)
            .map_err(|e| MeshError::SerializationError(format!("{}", e)))?;
        self.session
            .put(topic, bytes)
            .await
            .map_err(|e| MeshError::ZenohError(format!("put: {}", e)))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests — Integration tests per Phase 1 spec
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::{AgentRequest, AgentResponse};
    use std::time::Duration;

    /// Helper: create a fresh v4 UUID as `[u8; 16]`.
    fn make_uuid() -> [u8; 16] {
        *uuid::Uuid::new_v4().as_bytes()
    }

    /// Helper: build a sample `AgentRequest`.
    fn sample_request() -> AgentRequest {
        AgentRequest {
            request_id: make_uuid(),
            session_id: make_uuid(),
            user_id: String::from("test_user"),
            content: String::from("hello mesh"),
            timestamp_us: 1_700_000_000_000_000,
        }
    }

    /// Helper: build a sample `AgentResponse`.
    fn sample_response() -> AgentResponse {
        AgentResponse {
            request_id: make_uuid(),
            content: String::from("skill result payload"),
            skill_trace: vec![String::from("summarize")],
        }
    }

    // =====================================================================
    // Integration test: pub/sub AgentRequest across two peer sessions
    // =====================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn publish_subscribe_agent_request_round_trip() {
        // Create two separate peer-mode routers (in-memory discovery).
        let router_a = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("router A");
        let router_b = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("router B");

        // Give Zenoh time to discover peers.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Router B subscribes to the topic that Router A will publish on.
        let subscriber = router_b
            .session()
            .declare_subscriber("aria/gateway/telegram/inbound")
            .await
            .expect("subscriber");

        // Small delay to let subscriber registration propagate.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Router A publishes an AgentRequest.
        let original = sample_request();
        router_a
            .publish_inbound("telegram", &original)
            .await
            .expect("publish");

        // Router B receives it.
        let received = tokio::time::timeout(Duration::from_secs(5), subscriber.recv_async())
            .await
            .expect("timeout waiting for message")
            .expect("recv error");

        let payload_bytes: Vec<u8> = received.payload().to_bytes().to_vec();
        let decoded: AgentRequest = serde_json::from_slice(&payload_bytes).expect("deserialize");

        assert_eq!(original.request_id, decoded.request_id);
        assert_eq!(original.session_id, decoded.session_id);
        assert_eq!(original.user_id, decoded.user_id);
        assert_eq!(original.content, decoded.content);
        assert_eq!(original.timestamp_us, decoded.timestamp_us);

        // Cleanup
        router_a.close().await.expect("close A");
        router_b.close().await.expect("close B");
    }

    // =====================================================================
    // Integration test: subscribe_results wildcard receives AgentResponse
    // =====================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscribe_results_receives_agent_response() {
        let router = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("router");

        // Give session time to initialize.
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Subscribe to skill results.
        let mut rx = router.subscribe_results(16).await.expect("subscribe");

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Publish a response on a matching skill result topic.
        let original = sample_response();
        let bytes = serde_json::to_vec(&original).expect("serialize");
        router
            .session()
            .put("aria/skill/node_42/result", bytes)
            .await
            .expect("put");

        // Receive via the results channel.
        let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed")
            .expect("deser error");

        assert_eq!(original.request_id, received.request_id);
        assert_eq!(original.content, received.content);
        assert_eq!(original.skill_trace, received.skill_trace);

        router.close().await.expect("close");
    }

    // =====================================================================
    // Integration test: skill call publish + receive
    // =====================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn publish_skill_call_round_trip() {
        let router_pub = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("pub router");
        let router_sub = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("sub router");

        tokio::time::sleep(Duration::from_millis(500)).await;

        let subscriber = router_sub
            .session()
            .declare_subscriber("aria/skill/edge_node/call/summarize")
            .await
            .expect("subscriber");

        tokio::time::sleep(Duration::from_millis(200)).await;

        let req = sample_request();
        router_pub
            .publish_skill_call("edge_node", "summarize", &req)
            .await
            .expect("publish_skill_call");

        let received = tokio::time::timeout(Duration::from_secs(5), subscriber.recv_async())
            .await
            .expect("timeout")
            .expect("recv");

        let decoded: AgentRequest =
            serde_json::from_slice(&received.payload().to_bytes().to_vec()).expect("deserialize");

        assert_eq!(req.request_id, decoded.request_id);
        assert_eq!(req.content, decoded.content);

        router_pub.close().await.expect("close pub");
        router_sub.close().await.expect("close sub");
    }

    // =====================================================================
    // Failure test: timeout on no publisher (network partition simulation)
    // =====================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timeout_on_no_message() {
        let router = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("router");

        tokio::time::sleep(Duration::from_millis(300)).await;

        let subscriber = router
            .session()
            .declare_subscriber("aria/gateway/nonexistent/inbound")
            .await
            .expect("subscriber");

        // Attempt to receive with a short timeout — no publisher exists.
        let result =
            tokio::time::timeout(Duration::from_millis(500), subscriber.recv_async()).await;

        assert!(result.is_err(), "expected timeout, but received a message");

        router.close().await.expect("close");
    }

    // =====================================================================
    // Error handling: MeshError display and From impls
    // =====================================================================

    #[test]
    fn mesh_error_display() {
        let err = MeshError::ZenohError("connection refused".into());
        assert!(format!("{}", err).contains("zenoh error"));

        let err = MeshError::Timeout("5s elapsed".into());
        assert!(format!("{}", err).contains("timeout"));

        let err = MeshError::SessionClosed;
        assert!(format!("{}", err).contains("session closed"));
    }

    #[test]
    fn mesh_error_from_aria_error() {
        let aria_err = AriaError::SerializationError("bad data".into());
        let mesh_err: MeshError = aria_err.into();
        match mesh_err {
            MeshError::SerializationError(msg) => {
                assert!(msg.contains("bad data"));
            }
            _ => panic!("expected SerializationError"),
        }
    }

    // =====================================================================
    // Config defaults
    // =====================================================================

    #[test]
    fn default_config_is_peer_mode_no_endpoints() {
        let cfg = MeshConfig::default();
        assert!(cfg.peer_mode);
        assert!(cfg.connect_endpoints.is_empty());
    }
}
