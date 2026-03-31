use aria_core::{
    RobotStateSnapshot, RoboticsCommandContract, RoboticsSafetyEnvelope, Ros2BridgeDirective,
    Ros2BridgeProfile, Ros2BridgeTarget,
};

use crate::robotics_bridge::{compile_robotics_contract, RoboticsBridgeDirective};

pub fn compile_ros2_bridge_directive(
    contract: &RoboticsCommandContract,
    state: &RobotStateSnapshot,
    envelope: &RoboticsSafetyEnvelope,
    profile: &Ros2BridgeProfile,
    target: Ros2BridgeTarget,
) -> Result<Ros2BridgeDirective, String> {
    let directive = compile_robotics_contract(contract, state, envelope)?;
    let namespace = target
        .namespace_override
        .clone()
        .unwrap_or_else(|| profile.namespace.clone());
    Ok(Ros2BridgeDirective {
        command_topic: render_topic(&namespace, &profile.command_topic),
        telemetry_topic: render_topic(&namespace, &profile.telemetry_topic),
        image_topic: profile
            .image_topic
            .as_ref()
            .map(|topic| render_topic(&namespace, topic)),
        service_prefix: profile
            .service_prefix
            .as_ref()
            .map(|prefix| render_topic(&namespace, prefix)),
        target,
        payload: bridge_directive_payload(&directive),
    })
}

fn render_topic(namespace: &str, topic: &str) -> String {
    let ns = namespace.trim_matches('/');
    let leaf = topic.trim_start_matches('/');
    if ns.is_empty() {
        format!("/{}", leaf)
    } else {
        format!("/{}/{}", ns, leaf)
    }
}

fn bridge_directive_payload(directive: &RoboticsBridgeDirective) -> serde_json::Value {
    serde_json::to_value(directive).unwrap_or_else(|_| serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::{
        RobotStateSnapshot, RoboticsCommandContract, RoboticsExecutionMode, RoboticsIntentKind,
        RoboticsSafetyEnvelope,
    };

    fn sample_contract() -> RoboticsCommandContract {
        RoboticsCommandContract {
            intent_id: [2; 16],
            robot_id: "rover-1".into(),
            requested_by_agent: "robotics_ctrl".into(),
            kind: RoboticsIntentKind::ReportState,
            actuator_id: None,
            target_velocity: None,
            reason: "status".into(),
            execution_mode: RoboticsExecutionMode::Simulation,
            timestamp_us: 44,
        }
    }

    fn sample_state() -> RobotStateSnapshot {
        RobotStateSnapshot {
            robot_id: "rover-1".into(),
            battery_percent: 80,
            active_faults: Vec::new(),
            degraded_local_mode: false,
            last_heartbeat_us: 55,
        }
    }

    fn sample_envelope() -> RoboticsSafetyEnvelope {
        RoboticsSafetyEnvelope {
            max_abs_velocity: 0.2,
            allowed_actuator_ids: vec![1, 2],
            motion_requires_approval: false,
            allow_capture: true,
        }
    }

    fn sample_profile() -> Ros2BridgeProfile {
        Ros2BridgeProfile {
            profile_id: "ros2-sim".into(),
            display_name: "ROS2 Sim".into(),
            namespace: "/robots/rover-1".into(),
            command_topic: "cmd".into(),
            telemetry_topic: "telemetry".into(),
            image_topic: Some("camera".into()),
            service_prefix: Some("svc".into()),
            requires_approval: false,
            simulation_only: true,
        }
    }

    #[test]
    fn compile_ros2_bridge_directive_namespaces_topics() {
        let directive = compile_ros2_bridge_directive(
            &sample_contract(),
            &sample_state(),
            &sample_envelope(),
            &sample_profile(),
            Ros2BridgeTarget {
                profile_id: "ros2-sim".into(),
                robot_id: "rover-1".into(),
                namespace_override: None,
            },
        )
        .expect("ros2 directive");
        assert_eq!(directive.command_topic, "/robots/rover-1/cmd");
        assert_eq!(directive.telemetry_topic, "/robots/rover-1/telemetry");
        assert_eq!(directive.image_topic.as_deref(), Some("/robots/rover-1/camera"));
        assert_eq!(directive.service_prefix.as_deref(), Some("/robots/rover-1/svc"));
    }
}
