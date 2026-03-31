use aria_core::{
    ConstraintViolation, RobotRuntimeRecord, RobotStateSnapshot, RoboticsCommandContract,
    RoboticsExecutionMode, RoboticsIntentKind, RoboticsSafetyEnvelope, RoboticsSafetyEvent,
    RoboticsSimulationOutcome, RoboticsSimulationRecord, Ros2BridgeProfile, Ros2BridgeTarget,
};

use crate::robotics_bridge::{compile_robotics_contract, RoboticsBridgeDirective};
use crate::ros2_bridge::compile_ros2_bridge_directive;
use crate::runtime_store::RuntimeStore;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RoboticsSimulationFixture {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default = "default_robotics_agent_id")]
    pub agent_id: String,
    #[serde(default = "default_robotics_connection_kind")]
    pub connection_kind: String,
    pub contract: RoboticsCommandContract,
    pub state: RobotStateSnapshot,
    pub safety_envelope: RoboticsSafetyEnvelope,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Ros2SimulationFixture {
    #[serde(flatten)]
    pub robotics: RoboticsSimulationFixture,
    pub ros2_profile: Ros2BridgeProfile,
    #[serde(default)]
    pub namespace_override: Option<String>,
}

impl RoboticsSimulationFixture {
    pub fn from_json_str(text: &str) -> Result<Self, String> {
        let mut value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("parse robotics fixture failed: {}", e))?;
        if let Some(intent_id) = value
            .get_mut("contract")
            .and_then(|contract| contract.get_mut("intent_id"))
        {
            if let Some(intent_id_str) = intent_id.as_str() {
                let parsed = uuid::Uuid::parse_str(intent_id_str)
                    .map_err(|e| format!("invalid contract.intent_id: {}", e))?;
                *intent_id = serde_json::json!(parsed.as_bytes());
            }
        }
        serde_json::from_value(value)
            .map_err(|e| format!("parse robotics fixture failed: {}", e))
    }
}

impl Ros2SimulationFixture {
    pub fn from_json_str(text: &str) -> Result<Self, String> {
        let mut value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("parse ros2 fixture failed: {}", e))?;
        if let Some(intent_id) = value
            .get_mut("contract")
            .and_then(|contract| contract.get_mut("intent_id"))
        {
            if let Some(intent_id_str) = intent_id.as_str() {
                let parsed = uuid::Uuid::parse_str(intent_id_str)
                    .map_err(|e| format!("invalid contract.intent_id: {}", e))?;
                *intent_id = serde_json::json!(parsed.as_bytes());
            }
        }
        serde_json::from_value(value).map_err(|e| format!("parse ros2 fixture failed: {}", e))
    }
}

fn default_robotics_agent_id() -> String {
    "robotics_ctrl".into()
}

fn default_robotics_connection_kind() -> String {
    "simulation".into()
}

fn directive_to_json(directive: &RoboticsBridgeDirective) -> serde_json::Value {
    serde_json::to_value(directive).unwrap_or_else(|_| serde_json::json!({}))
}

fn evaluate_robotics_execution(
    contract: &RoboticsCommandContract,
    state: &RobotStateSnapshot,
    envelope: &RoboticsSafetyEnvelope,
) -> Result<(RoboticsBridgeDirective, Vec<RoboticsSafetyEvent>), (Vec<RoboticsSafetyEvent>, String, RoboticsSimulationOutcome)> {
    let timestamp_us = contract.timestamp_us;
    if contract.robot_id != state.robot_id {
        return Err((Vec::new(), "robotics contract robot_id does not match robot state".into(), RoboticsSimulationOutcome::Rejected));
    }
    if state.degraded_local_mode && matches!(contract.kind, RoboticsIntentKind::MoveActuator) {
        let event = RoboticsSafetyEvent::DegradedLocalModeEntered {
            robot_id: contract.robot_id.clone(),
            reason: "robot is in degraded local mode; motion intents are blocked".into(),
            timestamp_us,
        };
        return Err((vec![event], "robot is in degraded local mode; motion intents are blocked".into(), RoboticsSimulationOutcome::Rejected));
    }
    if matches!(contract.kind, RoboticsIntentKind::MoveActuator) && envelope.motion_requires_approval {
        let event = RoboticsSafetyEvent::ApprovalRequired {
            robot_id: contract.robot_id.clone(),
            reason: "motion intent requires human approval".into(),
            timestamp_us,
        };
        return Err((vec![event], "motion intent requires human approval".into(), RoboticsSimulationOutcome::ApprovalRequired));
    }
    if matches!(contract.kind, RoboticsIntentKind::MoveActuator) && !state.active_faults.is_empty() {
        let event = RoboticsSafetyEvent::CoastModeActivated {
            robot_id: contract.robot_id.clone(),
            reason: format!(
                "robot has active faults; motion is blocked ({})",
                state.active_faults.join(", ")
            ),
            timestamp_us,
        };
        return Err((vec![event], "robot has active faults; motion is blocked".into(), RoboticsSimulationOutcome::Rejected));
    }
    if matches!(contract.kind, RoboticsIntentKind::MoveActuator) {
        if let Some(actuator_id) = contract.actuator_id {
            if !envelope.allowed_actuator_ids.contains(&actuator_id) {
                let event = RoboticsSafetyEvent::ConstraintViolation(ConstraintViolation {
                    node_id: contract.robot_id.clone(),
                    motor_id: actuator_id,
                    requested_velocity: contract.target_velocity.unwrap_or_default(),
                    envelope_max: envelope.max_abs_velocity,
                    timestamp_us,
                });
                return Err((vec![event], "actuator is outside the safety envelope".into(), RoboticsSimulationOutcome::Rejected));
            }
        }
        if let (Some(actuator_id), Some(velocity)) = (contract.actuator_id, contract.target_velocity) {
            if velocity.abs() > envelope.max_abs_velocity {
                let event = RoboticsSafetyEvent::ConstraintViolation(ConstraintViolation {
                    node_id: contract.robot_id.clone(),
                    motor_id: actuator_id,
                    requested_velocity: velocity,
                    envelope_max: envelope.max_abs_velocity,
                    timestamp_us,
                });
                return Err((vec![event], "target velocity exceeds the safety envelope".into(), RoboticsSimulationOutcome::Rejected));
            }
        }
    }

    compile_robotics_contract(contract, state, envelope)
        .map(|directive| (directive, Vec::new()))
        .map_err(|reason| (Vec::new(), reason, RoboticsSimulationOutcome::Rejected))
}

pub fn execute_robotics_simulation(
    store: &RuntimeStore,
    fixture: RoboticsSimulationFixture,
) -> Result<RoboticsSimulationRecord, String> {
    let created_at_us = chrono::Utc::now().timestamp_micros() as u64;
    let session_id = fixture
        .session_id
        .as_deref()
        .map(|value| {
            uuid::Uuid::parse_str(value)
                .map(|parsed| *parsed.as_bytes())
                .map_err(|e| format!("invalid session_id: {}", e))
        })
        .transpose()?;

    let runtime_state = RobotRuntimeRecord {
        robot_id: fixture.state.robot_id.clone(),
        state: fixture.state.clone(),
        safety_envelope: fixture.safety_envelope.clone(),
        execution_mode: RoboticsExecutionMode::Simulation,
        connection_kind: fixture.connection_kind,
        bridge_profile_id: None,
        updated_at_us: created_at_us,
    };
    store.upsert_robot_runtime_state(&runtime_state, created_at_us)?;

    let simulation_id = format!("robot-sim-{}", uuid::Uuid::new_v4());
    match evaluate_robotics_execution(&fixture.contract, &fixture.state, &fixture.safety_envelope) {
        Ok((directive, safety_events)) => {
            let record = RoboticsSimulationRecord {
                simulation_id,
                session_id,
                agent_id: fixture.agent_id,
                robot_id: fixture.state.robot_id.clone(),
                contract: fixture.contract,
                state: fixture.state,
                safety_envelope: fixture.safety_envelope,
                outcome: RoboticsSimulationOutcome::Simulated,
                safety_events,
                ros2_profile_id: None,
                directive_json: Some(directive_to_json(&directive)),
                rejection_reason: None,
                created_at_us,
            };
            store.append_robotics_simulation(&record)?;
            Ok(record)
        }
        Err((safety_events, reason, outcome)) => {
            let record = RoboticsSimulationRecord {
                simulation_id,
                session_id,
                agent_id: fixture.agent_id,
                robot_id: fixture.state.robot_id.clone(),
                contract: fixture.contract,
                state: fixture.state,
                safety_envelope: fixture.safety_envelope,
                outcome,
                safety_events,
                ros2_profile_id: None,
                directive_json: None,
                rejection_reason: Some(reason),
                created_at_us,
            };
            store.append_robotics_simulation(&record)?;
            Ok(record)
        }
    }
}

pub fn execute_ros2_simulation(
    store: &RuntimeStore,
    fixture: Ros2SimulationFixture,
) -> Result<RoboticsSimulationRecord, String> {
    let created_at_us = chrono::Utc::now().timestamp_micros() as u64;
    let profile = fixture.ros2_profile;
    store.upsert_ros2_bridge_profile(&profile, created_at_us)?;
    let session_id = fixture
        .robotics
        .session_id
        .as_deref()
        .map(|value| {
            uuid::Uuid::parse_str(value)
                .map(|parsed| *parsed.as_bytes())
                .map_err(|e| format!("invalid session_id: {}", e))
        })
        .transpose()?;

    let runtime_state = RobotRuntimeRecord {
        robot_id: fixture.robotics.state.robot_id.clone(),
        state: fixture.robotics.state.clone(),
        safety_envelope: fixture.robotics.safety_envelope.clone(),
        execution_mode: RoboticsExecutionMode::Simulation,
        connection_kind: "ros2_simulation".into(),
        bridge_profile_id: Some(profile.profile_id.clone()),
        updated_at_us: created_at_us,
    };
    store.upsert_robot_runtime_state(&runtime_state, created_at_us)?;

    let simulation_id = format!("robot-sim-{}", uuid::Uuid::new_v4());
    let target = Ros2BridgeTarget {
        profile_id: profile.profile_id.clone(),
        robot_id: fixture.robotics.state.robot_id.clone(),
        namespace_override: fixture.namespace_override.clone(),
    };
    match evaluate_robotics_execution(
        &fixture.robotics.contract,
        &fixture.robotics.state,
        &fixture.robotics.safety_envelope,
    ) {
        Ok((_, safety_events)) => {
            let ros2_directive = compile_ros2_bridge_directive(
                &fixture.robotics.contract,
                &fixture.robotics.state,
                &fixture.robotics.safety_envelope,
                &profile,
                target,
            )?;
            let record = RoboticsSimulationRecord {
                simulation_id,
                session_id,
                agent_id: fixture.robotics.agent_id,
                robot_id: fixture.robotics.state.robot_id.clone(),
                contract: fixture.robotics.contract,
                state: fixture.robotics.state,
                safety_envelope: fixture.robotics.safety_envelope,
                outcome: RoboticsSimulationOutcome::Simulated,
                safety_events,
                ros2_profile_id: Some(profile.profile_id),
                directive_json: Some(serde_json::to_value(ros2_directive).unwrap_or_else(|_| serde_json::json!({}))),
                rejection_reason: None,
                created_at_us,
            };
            store.append_robotics_simulation(&record)?;
            Ok(record)
        }
        Err((safety_events, reason, outcome)) => {
            let record = RoboticsSimulationRecord {
                simulation_id,
                session_id,
                agent_id: fixture.robotics.agent_id,
                robot_id: fixture.robotics.state.robot_id.clone(),
                contract: fixture.robotics.contract,
                state: fixture.robotics.state,
                safety_envelope: fixture.robotics.safety_envelope,
                outcome,
                safety_events,
                ros2_profile_id: Some(profile.profile_id),
                directive_json: None,
                rejection_reason: Some(reason),
                created_at_us,
            };
            store.append_robotics_simulation(&record)?;
            Ok(record)
        }
    }
}
