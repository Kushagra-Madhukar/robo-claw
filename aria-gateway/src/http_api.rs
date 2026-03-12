/// Minimal route map for OpenAI-compatible HTTP stubs and gateway inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiRoute {
    ChatCompletions,
    Responses,
    Health,
    Agents,
    Tools,
    SessionInspect,
    SessionDelete,
    WebSocketEvents,
    UiAssets,
}

pub fn route_from_path(path: &str) -> Option<ApiRoute> {
    match path {
        "/v1/chat/completions" => Some(ApiRoute::ChatCompletions),
        "/v1/responses" => Some(ApiRoute::Responses),
        "/health" => Some(ApiRoute::Health),
        "/v1/agents" => Some(ApiRoute::Agents),
        "/v1/tools" => Some(ApiRoute::Tools),
        "/ws" => Some(ApiRoute::WebSocketEvents),
        p if p.starts_with("/v1/sessions/") => Some(ApiRoute::SessionInspect),
        p if p.starts_with("/ui/") => Some(ApiRoute::UiAssets),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_core_paths() {
        assert_eq!(
            route_from_path("/v1/chat/completions"),
            Some(ApiRoute::ChatCompletions)
        );
        assert_eq!(route_from_path("/health"), Some(ApiRoute::Health));
        assert_eq!(route_from_path("/ws"), Some(ApiRoute::WebSocketEvents));
    }
}
