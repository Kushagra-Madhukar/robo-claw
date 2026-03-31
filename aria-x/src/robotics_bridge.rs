#![allow(dead_code)]

use aria_core::{
    HardwareIntent, RobotStateSnapshot, RoboticsCommandContract, RoboticsIntentKind,
    RoboticsSafetyEnvelope,
};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RoboticsBridgeDirective {
    Hardware(Vec<HardwareIntent>),
    Observe {
        robot_id: String,
        actuator_id: Option<u8>,
        operation: String,
    },
    Report {
        robot_id: String,
    },
}

pub fn compile_robotics_contract(
    contract: &RoboticsCommandContract,
    state: &RobotStateSnapshot,
    envelope: &RoboticsSafetyEnvelope,
) -> Result<RoboticsBridgeDirective, String> {
    contract.validate().map_err(|e| e.to_string())?;

    if contract.robot_id != state.robot_id {
        return Err("robotics contract robot_id does not match robot state".to_string());
    }

    if state.degraded_local_mode && matches!(contract.kind, RoboticsIntentKind::MoveActuator) {
        return Err("robot is in degraded local mode; motion intents are blocked".to_string());
    }

    match contract.kind {
        RoboticsIntentKind::Halt => Ok(RoboticsBridgeDirective::Hardware(
            envelope
                .allowed_actuator_ids
                .iter()
                .map(|motor_id| HardwareIntent {
                    intent_id: 0,
                    motor_id: *motor_id,
                    target_velocity: 0.0,
                })
                .collect(),
        )),
        RoboticsIntentKind::InspectActuator => Ok(RoboticsBridgeDirective::Observe {
            robot_id: contract.robot_id.clone(),
            actuator_id: contract.actuator_id,
            operation: "inspect_actuator".to_string(),
        }),
        RoboticsIntentKind::CaptureImage => {
            if !envelope.allow_capture {
                return Err("capture is not permitted by the safety envelope".to_string());
            }
            Ok(RoboticsBridgeDirective::Observe {
                robot_id: contract.robot_id.clone(),
                actuator_id: None,
                operation: "capture_image".to_string(),
            })
        }
        RoboticsIntentKind::ReportState => Ok(RoboticsBridgeDirective::Report {
            robot_id: contract.robot_id.clone(),
        }),
        RoboticsIntentKind::MoveActuator => {
            if envelope.motion_requires_approval {
                return Err("motion intent requires human approval".to_string());
            }
            let actuator_id = contract
                .actuator_id
                .ok_or_else(|| "move_actuator missing actuator_id".to_string())?;
            if !envelope.allowed_actuator_ids.contains(&actuator_id) {
                return Err("actuator is outside the safety envelope".to_string());
            }
            let velocity = contract
                .target_velocity
                .ok_or_else(|| "move_actuator missing target_velocity".to_string())?;
            if velocity.abs() > envelope.max_abs_velocity {
                return Err("target velocity exceeds the safety envelope".to_string());
            }
            if !state.active_faults.is_empty() {
                return Err("robot has active faults; motion is blocked".to_string());
            }
            Ok(RoboticsBridgeDirective::Hardware(vec![HardwareIntent {
                intent_id: 0,
                motor_id: actuator_id,
                target_velocity: velocity,
            }]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::{RoboticsExecutionMode, Uuid};

    fn sample_uuid() -> Uuid {
        [7; 16]
    }

    fn sample_contract(kind: RoboticsIntentKind) -> RoboticsCommandContract {
        RoboticsCommandContract {
            intent_id: sample_uuid(),
            robot_id: "rover-1".into(),
            requested_by_agent: "robotics_ctrl".into(),
            kind,
            actuator_id: Some(4),
            target_velocity: Some(0.15),
            reason: "diagnostic".into(),
            execution_mode: RoboticsExecutionMode::Hardware,
            timestamp_us: 10,
        }
    }

    fn sample_state() -> RobotStateSnapshot {
        RobotStateSnapshot {
            robot_id: "rover-1".into(),
            battery_percent: 90,
            active_faults: Vec::new(),
            degraded_local_mode: false,
            last_heartbeat_us: 11,
        }
    }

    fn sample_envelope() -> RoboticsSafetyEnvelope {
        RoboticsSafetyEnvelope {
            max_abs_velocity: 0.2,
            allowed_actuator_ids: vec![1, 4, 7],
            motion_requires_approval: false,
            allow_capture: true,
        }
    }

    #[test]
    fn compile_robotics_contract_rejects_motion_outside_envelope() {
        let mut contract = sample_contract(RoboticsIntentKind::MoveActuator);
        contract.target_velocity = Some(0.5);
        let err = compile_robotics_contract(&contract, &sample_state(), &sample_envelope())
            .expect_err("expected rejection");
        assert!(err.contains("exceeds"));
    }

    #[test]
    fn compile_robotics_contract_halt_zeroes_allowed_actuators() {
        let mut contract = sample_contract(RoboticsIntentKind::Halt);
        contract.actuator_id = None;
        contract.target_velocity = None;
        let directive = compile_robotics_contract(&contract, &sample_state(), &sample_envelope())
            .expect("halt directive");
        match directive {
            RoboticsBridgeDirective::Hardware(intents) => {
                assert_eq!(intents.len(), 3);
                assert!(intents.iter().all(|intent| intent.target_velocity == 0.0));
            }
            other => panic!("expected hardware directive, got {:?}", other),
        }
    }

    #[test]
    fn compile_robotics_contract_inspect_returns_observe_directive() {
        let mut contract = sample_contract(RoboticsIntentKind::InspectActuator);
        contract.target_velocity = None;
        let directive = compile_robotics_contract(&contract, &sample_state(), &sample_envelope())
            .expect("inspect directive");
        match directive {
            RoboticsBridgeDirective::Observe {
                operation,
                actuator_id,
                ..
            } => {
                assert_eq!(operation, "inspect_actuator");
                assert_eq!(actuator_id, Some(4));
            }
            other => panic!("expected observe directive, got {:?}", other),
        }
    }

    #[test]
    fn compile_robotics_contract_blocks_motion_in_degraded_local_mode() {
        let contract = sample_contract(RoboticsIntentKind::MoveActuator);
        let mut state = sample_state();
        state.degraded_local_mode = true;
        let err = compile_robotics_contract(&contract, &state, &sample_envelope())
            .expect_err("expected degraded mode rejection");
        assert!(err.contains("degraded local mode"));
    }

    #[test]
    fn compile_robotics_contract_blocks_motion_when_approval_required() {
        let contract = sample_contract(RoboticsIntentKind::MoveActuator);
        let mut envelope = sample_envelope();
        envelope.motion_requires_approval = true;
        let err = compile_robotics_contract(&contract, &sample_state(), &envelope)
            .expect_err("expected approval rejection");
        assert!(err.contains("requires human approval"));
    }

    #[test]
    fn compile_robotics_contract_blocks_motion_on_active_faults() {
        let contract = sample_contract(RoboticsIntentKind::MoveActuator);
        let mut state = sample_state();
        state.active_faults.push("motor_overheat".into());
        let err = compile_robotics_contract(&contract, &state, &sample_envelope())
            .expect_err("expected active fault rejection");
        assert!(err.contains("active faults"));
    }
}
