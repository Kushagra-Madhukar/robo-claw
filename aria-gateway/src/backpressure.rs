use std::collections::VecDeque;

use aria_core::AgentRequest;

/// Semantic backpressure queue:
/// drop repeated adjacent requests with identical normalized content.
pub struct SemanticBackpressure {
    max_queue: usize,
    queue: VecDeque<AgentRequest>,
}

impl SemanticBackpressure {
    pub fn new(max_queue: usize) -> Self {
        Self {
            max_queue,
            queue: VecDeque::new(),
        }
    }

    pub fn push(&mut self, req: AgentRequest) {
        let should_drop = self
            .queue
            .back()
            .map(|prev| prev.content == req.content && prev.user_id == req.user_id)
            .unwrap_or(false);
        if should_drop {
            return;
        }
        if self.queue.len() >= self.max_queue {
            self.queue.pop_front();
        }
        self.queue.push_back(req);
    }

    pub fn pop(&mut self) -> Option<AgentRequest> {
        self.queue.pop_front()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use aria_core::{GatewayChannel, MessageContent};

    use super::*;

    fn req(text: &str) -> AgentRequest {
        AgentRequest {
            request_id: [0; 16],
            session_id: [0; 16],
            channel: GatewayChannel::Cli,
            user_id: "u1".into(),
            content: MessageContent::Text(text.into()),
            timestamp_us: 1,
        }
    }

    #[test]
    fn drops_redundant_adjacent_semantic_state() {
        let mut bp = SemanticBackpressure::new(8);
        bp.push(req("same"));
        bp.push(req("same"));
        assert_eq!(bp.len(), 1);
    }
}
