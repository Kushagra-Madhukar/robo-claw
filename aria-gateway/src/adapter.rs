use aria_core::AgentRequest;

use crate::GatewayError;

/// Async trait for inbound signal adapters.
#[async_trait::async_trait]
pub trait GatewayAdapter: Send + Sync {
    /// Receive and normalize the next inbound signal.
    async fn receive(&self) -> Result<AgentRequest, GatewayError>;
}
