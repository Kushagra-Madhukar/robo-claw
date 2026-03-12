//! ROS2 bridge interfaces and conversion layer.
//!
//! This module provides a minimal abstraction over ROS2-style messages so the
//! Gateway can normalize them into `AgentRequest` values without depending
//! on a specific ROS2 client library at this stage.

use aria_core::{AgentRequest, GatewayChannel, MessageContent};

/// Simplified representation of a ROS2 string message on a topic.
#[derive(Debug, Clone)]
pub struct Ros2StringMessage {
    pub topic: String,
    pub data: String,
}

/// Trait for a ROS2 bridge capable of subscribing to topics and delivering
/// normalized messages to the gateway.
pub trait Ros2Bridge: Send + Sync {
    /// Subscribe to a topic and start forwarding messages to the provided
    /// callback. Implementations may spawn background tasks internally.
    fn subscribe<F>(&self, topic: &str, handler: F) -> Result<(), String>
    where
        F: Fn(Ros2StringMessage) + Send + 'static;
}

/// Map a ROS2 topic into a logical ARIA-X channel.
fn channel_from_topic(topic: &str) -> GatewayChannel {
    if topic.starts_with("/ros2/companion") {
        GatewayChannel::Ros2
    } else {
        GatewayChannel::Unknown
    }
}

/// Normalize a ROS2 string message into an `AgentRequest`.
pub fn normalize_ros2_message(msg: Ros2StringMessage, user_id: &str) -> AgentRequest {
    AgentRequest {
        request_id: *uuid::Uuid::new_v4().as_bytes(),
        session_id: *uuid::Uuid::new_v4().as_bytes(),
        channel: channel_from_topic(&msg.topic),
        user_id: user_id.to_string(),
        content: MessageContent::Text(msg.data),
        timestamp_us: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ros2_message_sets_channel_and_content() {
        let msg = Ros2StringMessage {
            topic: "/ros2/companion/temperature".into(),
            data: "check temperature".into(),
        };
        let req = normalize_ros2_message(msg, "ros2-user");
        assert_eq!(req.user_id, "ros2-user");
        assert_eq!(
            req.content,
            MessageContent::Text("check temperature".into())
        );
        assert!(matches!(req.channel, GatewayChannel::Ros2));
    }

    #[test]
    fn normalize_ros2_message_unknown_topic_uses_unknown_channel() {
        let msg = Ros2StringMessage {
            topic: "/some/other/topic".into(),
            data: "payload".into(),
        };
        let req = normalize_ros2_message(msg, "user");
        assert!(matches!(req.channel, GatewayChannel::Unknown));
    }
}
