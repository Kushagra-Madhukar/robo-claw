//! # aria-mesh
//!
//! L4 Mesh Transport layer for HiveClaw.
//!
//! Provides [`ZenohRouter`] — a Zenoh-based pub/sub wrapper implementing
//! the HiveClaw topic hierarchy:
//!
//! | Pattern | Direction | Purpose |
//! |---------|-----------|---------|
//! | `aria/gateway/{channel}/inbound` | Publish | Ingress from gateway channels |
//! | `aria/skill/{node}/call/{skill}` | Publish | Dispatch skill invocations |
//! | `aria/skill/+/result` | Subscribe | Collect skill execution results |

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aria_core::{
    AgentRequest, AgentResponse, AriaError, RobotStateSnapshot, RoboticsCommandContract,
    RoboticsSafetyEvent,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Topic schema
// ---------------------------------------------------------------------------

/// Canonical HiveClaw Zenoh topic schema.
///
/// All mesh communication must use these helpers rather than hand-rolled
/// strings to ensure strict ACL enforcement and monitoring.
pub mod topics {
    pub const GATEWAY_INBOUND_PATTERN: &str = "aria/gateway/*/inbound";
    pub const SKILL_RESULT_PATTERN: &str = "aria/skill/*/result";
    pub const HEARTBEAT_PATTERN: &str = "aria/node/*/heartbeat";
    pub const ANNOUNCE_PATTERN: &str = "aria/node/*/announce";
    pub const COAST_MODE: &str = "aria/orchestrator/coast";
    pub const CONSTRAINT_VIOLATION: &str = "aria/safety/constraint_violation";
    pub const ROBOT_COMMAND_PATTERN: &str = "aria/robot/*/command";
    pub const ROBOT_STATE_PATTERN: &str = "aria/robot/*/state";
    pub const ROBOT_SAFETY_PATTERN: &str = "aria/robot/*/safety";

    pub fn gateway_inbound(channel: &str) -> String {
        format!("aria/gateway/{}/inbound", channel)
    }
    pub fn skill_call(node: &str, skill: &str) -> String {
        format!("aria/skill/{}/call/{}", node, skill)
    }
    pub fn skill_result(node: &str) -> String {
        format!("aria/skill/{}/result", node)
    }
    pub fn heartbeat(node_id: &str) -> String {
        format!("aria/node/{}/heartbeat", node_id)
    }
    pub fn announce(node_id: &str) -> String {
        format!("aria/node/{}/announce", node_id)
    }
    pub fn robot_command(robot_id: &str) -> String {
        format!("aria/robot/{}/command", robot_id)
    }
    pub fn robot_state(robot_id: &str) -> String {
        format!("aria/robot/{}/state", robot_id)
    }
    pub fn robot_safety(robot_id: &str) -> String {
        format!("aria/robot/{}/safety", robot_id)
    }
}

// ---------------------------------------------------------------------------
// Node roles and announcement
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    Orchestrator,
    Companion,
    Relay,
    Micro,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Healthy,
    Degraded,
    CoastMode,
}

/// Announcement published by a node on startup and periodically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAnnouncement {
    pub node_id: String,
    pub role: NodeRole,
    pub capabilities: Vec<String>,
    pub timestamp_us: u64,
}

/// Heartbeat event published by a node at regular intervals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatEvent {
    pub node_id: String,
    pub timestamp_us: u64,
    pub status: NodeStatus,
}

// ---------------------------------------------------------------------------
// HeartbeatMonitor — coast-mode detection
// ---------------------------------------------------------------------------

/// Tracks heartbeats from remote nodes and detects partition / timeout.
pub struct HeartbeatMonitor {
    /// How long without a heartbeat before declaring a node timed-out.
    pub timeout_ms: u64,
    last_seen: Mutex<HashMap<String, Instant>>,
}

impl HeartbeatMonitor {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            timeout_ms,
            last_seen: Mutex::new(HashMap::new()),
        }
    }

    /// Record a heartbeat received from `node_id` right now.
    pub fn record(&self, node_id: &str) {
        let mut guard = self.last_seen.lock().expect("heartbeat lock poisoned");
        guard.insert(node_id.to_string(), Instant::now());
    }

    /// Returns `true` if the node has not sent a heartbeat within `timeout_ms`.
    pub fn is_timed_out(&self, node_id: &str) -> bool {
        let guard = self.last_seen.lock().expect("heartbeat lock poisoned");
        match guard.get(node_id) {
            None => true,
            Some(t) => t.elapsed().as_millis() as u64 >= self.timeout_ms,
        }
    }

    /// Returns all node IDs that are currently considered timed-out.
    pub fn timed_out_nodes(&self) -> Vec<String> {
        let guard = self.last_seen.lock().expect("heartbeat lock poisoned");
        guard
            .iter()
            .filter(|(_, t)| t.elapsed().as_millis() as u64 >= self.timeout_ms)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Determine if coast-mode should be activated: orchestrator is partitioned.
    pub fn should_activate_coast_mode(&self, orchestrator_id: &str) -> bool {
        self.is_timed_out(orchestrator_id)
    }
}

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
    /// Topic ACL denied a publish/subscribe attempt for the given node role.
    Unauthorized(String),
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshError::ZenohError(msg) => write!(f, "zenoh error: {}", msg),
            MeshError::SerializationError(msg) => write!(f, "serialization error: {}", msg),
            MeshError::Timeout(msg) => write!(f, "timeout: {}", msg),
            MeshError::SessionClosed => write!(f, "session closed"),
            MeshError::Unauthorized(msg) => write!(f, "unauthorized: {}", msg),
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
    /// Logical node identifier (for monitoring and ACLs).
    pub node_id: String,
    /// Role of this node for topic ACLs.
    pub node_role: NodeRole,
    /// Optional CA certificate path for mTLS.
    pub tls_ca_cert: Option<String>,
    /// Optional client certificate path for mTLS.
    pub tls_cert: Option<String>,
    /// Optional client private key path for mTLS.
    pub tls_key: Option<String>,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            connect_endpoints: Vec::new(),
            peer_mode: true,
            node_id: "orchestrator".to_string(),
            node_role: NodeRole::Orchestrator,
            tls_ca_cert: None,
            tls_cert: None,
            tls_key: None,
        }
    }
}

/// Zenoh-based pub/sub router for the HiveClaw L4 mesh transport layer.
///
/// Wraps a [`zenoh::Session`] and provides topic-aware publish/subscribe
/// methods following the HiveClaw key hierarchy.
pub struct ZenohRouter {
    config: MeshConfig,
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

        // Optional mTLS configuration: CA + client cert/key.
        if let Some(ref ca_path) = config.tls_ca_cert {
            zenoh_config
                .insert_json5("transport/quic/ca", &format!(r#""{}""#, ca_path))
                .map_err(|e| MeshError::ZenohError(format!("config ca: {}", e)))?;
        }
        if let Some(ref cert_path) = config.tls_cert {
            zenoh_config
                .insert_json5("transport/quic/cert", &format!(r#""{}""#, cert_path))
                .map_err(|e| MeshError::ZenohError(format!("config cert: {}", e)))?;
        }
        if let Some(ref key_path) = config.tls_key {
            zenoh_config
                .insert_json5("transport/quic/key", &format!(r#""{}""#, key_path))
                .map_err(|e| MeshError::ZenohError(format!("config key: {}", e)))?;
        }

        let session = zenoh::open(zenoh_config)
            .await
            .map_err(|e| MeshError::ZenohError(format!("open: {}", e)))?;

        Ok(Self {
            config,
            session: Arc::new(session),
        })
    }

    /// Create a router from an existing Zenoh session (useful for testing).
    pub fn from_session(session: Arc<zenoh::Session>) -> Self {
        Self {
            config: MeshConfig::default(),
            session,
        }
    }

    /// Attempt to reconnect the underlying Zenoh session using the stored
    /// configuration. Useful after network partitions or restarts.
    pub async fn reconnect(&mut self) -> Result<(), MeshError> {
        let mut zenoh_config = zenoh::Config::default();

        if self.config.peer_mode {
            zenoh_config
                .insert_json5("mode", r#""peer""#)
                .map_err(|e| MeshError::ZenohError(format!("config mode: {}", e)))?;
        } else {
            zenoh_config
                .insert_json5("mode", r#""client""#)
                .map_err(|e| MeshError::ZenohError(format!("config mode: {}", e)))?;
        }

        if !self.config.connect_endpoints.is_empty() {
            let endpoints_json = serde_json::to_string(&self.config.connect_endpoints)
                .map_err(|e| MeshError::SerializationError(format!("{}", e)))?;
            zenoh_config
                .insert_json5("connect/endpoints", &endpoints_json)
                .map_err(|e| MeshError::ZenohError(format!("config endpoints: {}", e)))?;
        }

        if let Some(ref ca_path) = self.config.tls_ca_cert {
            zenoh_config
                .insert_json5("transport/quic/ca", &format!(r#""{}""#, ca_path))
                .map_err(|e| MeshError::ZenohError(format!("config ca: {}", e)))?;
        }
        if let Some(ref cert_path) = self.config.tls_cert {
            zenoh_config
                .insert_json5("transport/quic/cert", &format!(r#""{}""#, cert_path))
                .map_err(|e| MeshError::ZenohError(format!("config cert: {}", e)))?;
        }
        if let Some(ref key_path) = self.config.tls_key {
            zenoh_config
                .insert_json5("transport/quic/key", &format!(r#""{}""#, key_path))
                .map_err(|e| MeshError::ZenohError(format!("config key: {}", e)))?;
        }

        let session = zenoh::open(zenoh_config)
            .await
            .map_err(|e| MeshError::ZenohError(format!("reconnect open: {}", e)))?;
        self.session = Arc::new(session);
        Ok(())
    }

    fn check_acl(&self, topic: &str) -> Result<(), MeshError> {
        let role = self.config.node_role;
        let allowed = match role {
            NodeRole::Orchestrator => true,
            NodeRole::Companion => {
                topic.starts_with("aria/gateway/")
                    || topic.starts_with("aria/robot/")
                    || topic.contains("/announce")
                    || topic.contains("/heartbeat")
                    || topic.contains("/result")
            }
            NodeRole::Relay | NodeRole::Micro => {
                topic.starts_with("aria/skill/")
                    || topic.starts_with("aria/robot/")
                    || topic.contains("/heartbeat")
                    || topic.starts_with(topics::CONSTRAINT_VIOLATION)
            }
        };
        if allowed {
            Ok(())
        } else {
            Err(MeshError::Unauthorized(format!(
                "role {:?} cannot publish to topic '{}'",
                role, topic
            )))
        }
    }

    /// Publish an [`AgentRequest`] to the gateway inbound topic.
    ///
    /// Topic: `aria/gateway/{channel}/inbound`
    pub async fn publish_inbound(
        &self,
        channel: &str,
        request: &AgentRequest,
    ) -> Result<(), MeshError> {
        let topic = topics::gateway_inbound(channel);
        self.check_acl(&topic)?;
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
        let topic = topics::skill_call(node, skill);
        self.check_acl(&topic)?;
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
            .declare_subscriber(topics::SKILL_RESULT_PATTERN)
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

    /// Publish a node announcement.
    ///
    /// Topic: `aria/node/{node_id}/announce`
    pub async fn publish_announce(&self, announcement: &NodeAnnouncement) -> Result<(), MeshError> {
        let topic = topics::announce(&announcement.node_id);
        self.check_acl(&topic)?;
        self.publish(&topic, announcement).await
    }

    /// Publish a heartbeat event.
    ///
    /// Topic: `aria/node/{node_id}/heartbeat`
    pub async fn publish_heartbeat(&self, event: &HeartbeatEvent) -> Result<(), MeshError> {
        let topic = topics::heartbeat(&event.node_id);
        self.check_acl(&topic)?;
        self.publish(&topic, event).await
    }

    /// Subscribe to all heartbeat events across all nodes.
    ///
    /// Returns an `mpsc::Receiver<HeartbeatEvent>` that yields events as
    /// they arrive. Invalid payloads are silently dropped.
    pub async fn subscribe_heartbeats(
        &self,
        buffer_size: usize,
    ) -> Result<mpsc::Receiver<HeartbeatEvent>, MeshError> {
        let (tx, rx) = mpsc::channel(buffer_size);
        let subscriber = self
            .session
            .declare_subscriber(topics::HEARTBEAT_PATTERN)
            .await
            .map_err(|e| MeshError::ZenohError(format!("subscribe heartbeat: {}", e)))?;

        tokio::spawn(async move {
            while let Ok(sample) = subscriber.recv_async().await {
                let payload_bytes: Vec<u8> = sample.payload().to_bytes().to_vec();
                if let Ok(event) = serde_json::from_slice::<HeartbeatEvent>(&payload_bytes) {
                    if tx.send(event).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    /// Subscribe to node announcements.
    ///
    /// Returns an `mpsc::Receiver<NodeAnnouncement>` channel.
    pub async fn subscribe_announcements(
        &self,
        buffer_size: usize,
    ) -> Result<mpsc::Receiver<NodeAnnouncement>, MeshError> {
        let (tx, rx) = mpsc::channel(buffer_size);
        let subscriber = self
            .session
            .declare_subscriber(topics::ANNOUNCE_PATTERN)
            .await
            .map_err(|e| MeshError::ZenohError(format!("subscribe announce: {}", e)))?;

        tokio::spawn(async move {
            while let Ok(sample) = subscriber.recv_async().await {
                let payload_bytes: Vec<u8> = sample.payload().to_bytes().to_vec();
                if let Ok(ann) = serde_json::from_slice::<NodeAnnouncement>(&payload_bytes) {
                    if tx.send(ann).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    /// Publish a coast-mode activation signal.
    ///
    /// Called by a companion/relay node when the orchestrator is unreachable.
    /// Topic: `aria/orchestrator/coast`
    pub async fn publish_coast_mode(&self, reason: &str) -> Result<(), MeshError> {
        let bytes = serde_json::to_vec(&serde_json::json!({ "reason": reason }))
            .map_err(|e| MeshError::SerializationError(format!("{}", e)))?;
        self.check_acl(topics::COAST_MODE)?;
        self.session
            .put(topics::COAST_MODE, bytes)
            .await
            .map_err(|e| MeshError::ZenohError(format!("coast-mode put: {}", e)))?;
        Ok(())
    }

    /// Publish a `ConstraintViolation` event on the safety topic.
    pub async fn publish_constraint_violation(
        &self,
        violation: &aria_core::ConstraintViolation,
    ) -> Result<(), MeshError> {
        let bytes = serde_json::to_vec(violation)
            .map_err(|e| MeshError::SerializationError(format!("{}", e)))?;
        self.check_acl(topics::CONSTRAINT_VIOLATION)?;
        self.session
            .put(topics::CONSTRAINT_VIOLATION, bytes)
            .await
            .map_err(|e| MeshError::ZenohError(format!("constraint_violation put: {}", e)))?;
        Ok(())
    }

    pub async fn publish_robot_command(
        &self,
        command: &RoboticsCommandContract,
    ) -> Result<(), MeshError> {
        let topic = topics::robot_command(&command.robot_id);
        self.check_acl(&topic)?;
        self.publish(&topic, command).await
    }

    pub async fn publish_robot_state(
        &self,
        snapshot: &RobotStateSnapshot,
    ) -> Result<(), MeshError> {
        let topic = topics::robot_state(&snapshot.robot_id);
        self.check_acl(&topic)?;
        self.publish(&topic, snapshot).await
    }

    pub async fn publish_robot_safety_event(
        &self,
        robot_id: &str,
        event: &RoboticsSafetyEvent,
    ) -> Result<(), MeshError> {
        let topic = topics::robot_safety(robot_id);
        self.check_acl(&topic)?;
        self.publish(&topic, event).await
    }

    pub async fn subscribe_robot_states(
        &self,
        buffer_size: usize,
    ) -> Result<mpsc::Receiver<RobotStateSnapshot>, MeshError> {
        let (tx, rx) = mpsc::channel(buffer_size);
        let subscriber = self
            .session
            .declare_subscriber(topics::ROBOT_STATE_PATTERN)
            .await
            .map_err(|e| MeshError::ZenohError(format!("subscribe robot state: {}", e)))?;

        tokio::spawn(async move {
            while let Ok(sample) = subscriber.recv_async().await {
                let payload_bytes: Vec<u8> = sample.payload().to_bytes().to_vec();
                if let Ok(snapshot) = serde_json::from_slice::<RobotStateSnapshot>(&payload_bytes) {
                    if tx.send(snapshot).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(rx)
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
    use aria_core::{
        AgentRequest, AgentResponse, GatewayChannel, MessageContent, PolicyDecision,
        RobotStateSnapshot, RoboticsCommandContract, RoboticsExecutionMode, RoboticsIntentKind,
        RoboticsSafetyEvent, SkillExecutionRecord,
    };
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
            channel: GatewayChannel::Telegram,
            user_id: String::from("test_user"),
            content: MessageContent::Text(String::from("hello mesh")),
            tool_runtime_policy: None,
            timestamp_us: 1_700_000_000_000_000,
        }
    }

    /// Helper: build a sample `AgentResponse`.
    fn sample_response() -> AgentResponse {
        AgentResponse {
            request_id: make_uuid(),
            content: MessageContent::Text(String::from("skill result payload")),
            skill_trace: vec![SkillExecutionRecord {
                tool_name: String::from("summarize"),
                arguments_json: String::from("{}"),
                result_summary: String::from("ok"),
                duration_ms: 3,
                policy_decision: PolicyDecision::Allow,
            }],
            latency_ms: 18,
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
            serde_json::from_slice(&received.payload().to_bytes()).expect("deserialize");

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

        let err = MeshError::Unauthorized("no access".into());
        assert!(format!("{}", err).contains("unauthorized"));
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
        assert_eq!(cfg.node_id, "orchestrator");
        assert!(matches!(cfg.node_role, NodeRole::Orchestrator));
    }

    // =====================================================================
    // Topic schema helpers
    // =====================================================================

    #[test]
    fn topic_schema_gateway_inbound() {
        assert_eq!(
            topics::gateway_inbound("telegram"),
            "aria/gateway/telegram/inbound"
        );
    }

    #[test]
    fn topic_schema_skill_call() {
        assert_eq!(
            topics::skill_call("edge_01", "summarize"),
            "aria/skill/edge_01/call/summarize"
        );
    }

    #[test]
    fn topic_schema_skill_result() {
        assert_eq!(topics::skill_result("node_42"), "aria/skill/node_42/result");
    }

    #[test]
    fn topic_schema_heartbeat() {
        assert_eq!(
            topics::heartbeat("orchestrator-01"),
            "aria/node/orchestrator-01/heartbeat"
        );
    }

    #[test]
    fn topic_schema_announce() {
        assert_eq!(
            topics::announce("companion-02"),
            "aria/node/companion-02/announce"
        );
    }

    #[test]
    fn topic_schema_robotics_topics() {
        assert_eq!(
            topics::robot_command("rover-1"),
            "aria/robot/rover-1/command"
        );
        assert_eq!(topics::robot_state("rover-1"), "aria/robot/rover-1/state");
        assert_eq!(topics::robot_safety("rover-1"), "aria/robot/rover-1/safety");
    }

    // =====================================================================
    // HeartbeatMonitor unit tests
    // =====================================================================

    #[test]
    fn heartbeat_monitor_new_node_is_timed_out() {
        let monitor = HeartbeatMonitor::new(5_000);
        assert!(
            monitor.is_timed_out("ghost_node"),
            "a node never seen should be timed out"
        );
    }

    #[test]
    fn heartbeat_monitor_just_recorded_is_not_timed_out() {
        let monitor = HeartbeatMonitor::new(5_000);
        monitor.record("orch-01");
        assert!(
            !monitor.is_timed_out("orch-01"),
            "just-recorded node must not time out immediately"
        );
    }

    #[test]
    fn heartbeat_monitor_coast_mode_when_orch_missing() {
        let monitor = HeartbeatMonitor::new(5_000);
        assert!(
            monitor.should_activate_coast_mode("orch-01"),
            "orchestrator never seen → coast mode"
        );
    }

    #[test]
    fn heartbeat_monitor_no_coast_mode_when_orch_alive() {
        let monitor = HeartbeatMonitor::new(5_000);
        monitor.record("orch-01");
        assert!(
            !monitor.should_activate_coast_mode("orch-01"),
            "orchestrator just heartbeated → no coast mode"
        );
    }

    #[test]
    fn heartbeat_monitor_timed_out_nodes_list() {
        let monitor = HeartbeatMonitor::new(0); // 0ms = instant timeout
        monitor.record("dead-01");
        monitor.record("dead-02");
        std::thread::sleep(std::time::Duration::from_millis(1));
        let timed_out = monitor.timed_out_nodes();
        assert!(timed_out.contains(&"dead-01".to_string()));
        assert!(timed_out.contains(&"dead-02".to_string()));
    }

    // =====================================================================
    // Integration: heartbeat publish/subscribe round-trip
    // =====================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn heartbeat_pub_sub_round_trip() {
        let router_pub = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("pub router");
        let router_sub = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("sub router");

        tokio::time::sleep(Duration::from_millis(500)).await;

        let mut rx = router_sub
            .subscribe_heartbeats(8)
            .await
            .expect("subscribe heartbeats");

        tokio::time::sleep(Duration::from_millis(200)).await;

        let event = HeartbeatEvent {
            node_id: "orchestrator-test".into(),
            timestamp_us: 1_700_000_000_000_000,
            status: NodeStatus::Healthy,
        };
        router_pub
            .publish_heartbeat(&event)
            .await
            .expect("publish heartbeat");

        let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed");

        assert_eq!(received.node_id, "orchestrator-test");
        assert_eq!(received.status, NodeStatus::Healthy);

        router_pub.close().await.ok();
        router_sub.close().await.ok();
    }

    // =====================================================================
    // Integration: node announce publish/subscribe round-trip
    // =====================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn announce_pub_sub_round_trip() {
        let router_pub = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("pub router");
        let router_sub = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("sub router");

        tokio::time::sleep(Duration::from_millis(500)).await;

        let mut rx = router_sub
            .subscribe_announcements(8)
            .await
            .expect("subscribe announcements");

        tokio::time::sleep(Duration::from_millis(200)).await;

        let ann = NodeAnnouncement {
            node_id: "relay-edge-01".into(),
            role: NodeRole::Relay,
            capabilities: vec!["sensor_read".into(), "wasm_exec".into()],
            timestamp_us: 1_700_000_000_000_001,
        };
        router_pub
            .publish_announce(&ann)
            .await
            .expect("publish announce");

        let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed");

        assert_eq!(received.node_id, "relay-edge-01");
        assert_eq!(received.role, NodeRole::Relay);
        assert!(received.capabilities.contains(&"sensor_read".to_string()));

        router_pub.close().await.ok();
        router_sub.close().await.ok();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn robot_state_pub_sub_round_trip() {
        let router_pub = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("pub router");
        let router_sub = ZenohRouter::new(MeshConfig::default())
            .await
            .expect("sub router");

        tokio::time::sleep(Duration::from_millis(500)).await;

        let mut rx = router_sub
            .subscribe_robot_states(8)
            .await
            .expect("subscribe robot states");

        tokio::time::sleep(Duration::from_millis(200)).await;

        let snapshot = RobotStateSnapshot {
            robot_id: "rover-1".into(),
            battery_percent: 87,
            active_faults: vec![],
            degraded_local_mode: false,
            last_heartbeat_us: 123,
        };
        router_pub
            .publish_robot_state(&snapshot)
            .await
            .expect("publish robot state");

        let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed");

        assert_eq!(received, snapshot);

        router_pub.close().await.ok();
        router_sub.close().await.ok();
    }

    #[test]
    fn robotics_payloads_serialize_for_mesh_transport() {
        let command = RoboticsCommandContract {
            intent_id: make_uuid(),
            robot_id: "rover-1".into(),
            requested_by_agent: "robotics_ctrl".into(),
            kind: RoboticsIntentKind::InspectActuator,
            actuator_id: Some(2),
            target_velocity: None,
            reason: "inspection".into(),
            execution_mode: RoboticsExecutionMode::Hardware,
            timestamp_us: 88,
        };
        let safety = RoboticsSafetyEvent::CoastModeActivated {
            robot_id: "rover-1".into(),
            reason: "orchestrator timeout".into(),
            timestamp_us: 99,
        };
        assert!(serde_json::to_vec(&command).is_ok());
        assert!(serde_json::to_vec(&safety).is_ok());
    }
}
