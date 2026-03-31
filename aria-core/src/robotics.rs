use super::*;

/// Deterministic runtime mode for robotics execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoboticsExecutionMode {
    Simulation,
    Hardware,
    DegradedLocal,
}

/// High-level robotics intent type emitted by planning layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoboticsIntentKind {
    Halt,
    InspectActuator,
    MoveActuator,
    CaptureImage,
    ReportState,
}

/// High-level robotics contract that planning layers may emit.
///
/// This remains intentionally more abstract than [`HardwareIntent`]:
/// LLM-facing layers describe bounded intent, and a deterministic bridge
/// decides whether that intent may translate into low-level actuation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoboticsCommandContract {
    pub intent_id: Uuid,
    pub robot_id: String,
    pub requested_by_agent: String,
    pub kind: RoboticsIntentKind,
    pub actuator_id: Option<u8>,
    pub target_velocity: Option<f32>,
    pub reason: String,
    pub execution_mode: RoboticsExecutionMode,
    pub timestamp_us: u64,
}

/// Snapshot of robot runtime state used by deterministic executors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RobotStateSnapshot {
    pub robot_id: String,
    pub battery_percent: u8,
    pub active_faults: Vec<String>,
    pub degraded_local_mode: bool,
    pub last_heartbeat_us: u64,
}

/// Deterministic safety envelope applied before any low-level actuation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoboticsSafetyEnvelope {
    pub max_abs_velocity: f32,
    pub allowed_actuator_ids: Vec<u8>,
    pub motion_requires_approval: bool,
    pub allow_capture: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ros2BridgeProfile {
    pub profile_id: String,
    pub display_name: String,
    pub namespace: String,
    pub command_topic: String,
    pub telemetry_topic: String,
    #[serde(default)]
    pub image_topic: Option<String>,
    #[serde(default)]
    pub service_prefix: Option<String>,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub simulation_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ros2BridgeTarget {
    pub profile_id: String,
    pub robot_id: String,
    #[serde(default)]
    pub namespace_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ros2BridgeDirective {
    pub target: Ros2BridgeTarget,
    pub command_topic: String,
    pub telemetry_topic: String,
    #[serde(default)]
    pub image_topic: Option<String>,
    #[serde(default)]
    pub service_prefix: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RobotRuntimeRecord {
    pub robot_id: String,
    pub state: RobotStateSnapshot,
    pub safety_envelope: RoboticsSafetyEnvelope,
    pub execution_mode: RoboticsExecutionMode,
    pub connection_kind: String,
    #[serde(default)]
    pub bridge_profile_id: Option<String>,
    pub updated_at_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoboticsSimulationOutcome {
    Simulated,
    ApprovalRequired,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoboticsSimulationRecord {
    pub simulation_id: String,
    pub session_id: Option<Uuid>,
    pub agent_id: String,
    pub robot_id: String,
    pub contract: RoboticsCommandContract,
    pub state: RobotStateSnapshot,
    pub safety_envelope: RoboticsSafetyEnvelope,
    pub outcome: RoboticsSimulationOutcome,
    #[serde(default)]
    pub safety_events: Vec<RoboticsSafetyEvent>,
    #[serde(default)]
    pub ros2_profile_id: Option<String>,
    #[serde(default)]
    pub directive_json: Option<serde_json::Value>,
    #[serde(default)]
    pub rejection_reason: Option<String>,
    pub created_at_us: u64,
}

/// Safety events that may be emitted over the robotics control plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RoboticsSafetyEvent {
    ConstraintViolation(ConstraintViolation),
    ApprovalRequired {
        robot_id: String,
        reason: String,
        timestamp_us: u64,
    },
    DegradedLocalModeEntered {
        robot_id: String,
        reason: String,
        timestamp_us: u64,
    },
    CoastModeActivated {
        robot_id: String,
        reason: String,
        timestamp_us: u64,
    },
}

/// Low-level hardware actuator command for the HAL layer.
///
/// This struct is intentionally small and fixed-size so it can be
/// serialized without dynamic allocation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HardwareIntent {
    /// Unique identifier for this intent batch.
    pub intent_id: u32,
    /// Target motor/actuator identifier (0–255).
    pub motor_id: u8,
    /// Desired velocity set-point.
    pub target_velocity: f32,
}

// ---------------------------------------------------------------------------
// Helper constructors (available in std-enabled test / downstream crates)
// ---------------------------------------------------------------------------

impl AgentRequest {
    /// Validate that a `Uuid` field contains a plausible value
    /// (not all-zeros).
    pub fn validate_uuid(id: &Uuid) -> Result<(), AriaError> {
        if id.iter().all(|&b| b == 0) {
            return Err(AriaError::ValidationError(String::from(
                "UUID must not be all zeros",
            )));
        }
        Ok(())
    }
}

impl HardwareIntent {
    /// Serialize to postcard bytes without requiring std.
    pub fn to_postcard_bytes(&self) -> Result<Vec<u8>, AriaError> {
        postcard::to_allocvec(self)
            .map_err(|e| AriaError::SerializationError(alloc::format!("{}", e)))
    }

    /// Deserialize from postcard bytes.
    pub fn from_postcard_bytes(bytes: &[u8]) -> Result<Self, AriaError> {
        postcard::from_bytes(bytes)
            .map_err(|e| AriaError::SerializationError(alloc::format!("{}", e)))
    }
}

impl RoboticsCommandContract {
    pub fn validate(&self) -> Result<(), AriaError> {
        match self.kind {
            RoboticsIntentKind::Halt => {
                if self.target_velocity.is_some() {
                    return Err(AriaError::ValidationError(String::from(
                        "halt intent must not include target_velocity",
                    )));
                }
            }
            RoboticsIntentKind::InspectActuator => {
                if self.actuator_id.is_none() {
                    return Err(AriaError::ValidationError(String::from(
                        "inspect_actuator intent requires actuator_id",
                    )));
                }
                if self.target_velocity.is_some() {
                    return Err(AriaError::ValidationError(String::from(
                        "inspect_actuator intent must not include target_velocity",
                    )));
                }
            }
            RoboticsIntentKind::MoveActuator => {
                if self.actuator_id.is_none() {
                    return Err(AriaError::ValidationError(String::from(
                        "move_actuator intent requires actuator_id",
                    )));
                }
                if self.target_velocity.is_none() {
                    return Err(AriaError::ValidationError(String::from(
                        "move_actuator intent requires target_velocity",
                    )));
                }
            }
            RoboticsIntentKind::CaptureImage | RoboticsIntentKind::ReportState => {
                if self.actuator_id.is_some() || self.target_velocity.is_some() {
                    return Err(AriaError::ValidationError(String::from(
                        "observe/report intents must not include actuator motion fields",
                    )));
                }
            }
        }
        if self.robot_id.is_empty() {
            return Err(AriaError::ValidationError(String::from(
                "robot_id must not be empty",
            )));
        }
        if self.requested_by_agent.is_empty() {
            return Err(AriaError::ValidationError(String::from(
                "requested_by_agent must not be empty",
            )));
        }
        Ok(())
    }
}
