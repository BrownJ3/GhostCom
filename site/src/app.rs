use crate::page::INDEX_HTML;
use crate::relay::{RelayState, relay_ws};
use crate::rendezvous::{RendezvousState, rendezvous_ws};
use axum::{
    Router,
    http::{StatusCode, header},
    response::{Html, IntoResponse},
    routing::get,
};

pub fn app() -> Router {
    let public_routes = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .fallback(not_found);

    let rendezvous_routes = Router::new()
        .route("/rv", get(rendezvous_ws))
        .with_state(RendezvousState::default());

    let relay_routes = Router::new()
        .route("/relay", get(relay_ws))
        .with_state(RelayState::default());

    public_routes.merge(rendezvous_routes).merge(relay_routes)
}

async fn index() -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "public, max-age=300")],
        Html(INDEX_HTML),
    )
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "not found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn health_returns_ok() {
        let response = health().await.into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn index_is_minimal_connecting_screen() {
        let response = index().await.into_response();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();

        assert!(body.contains("Connecting"));
        assert!(body.contains("<span>.</span><span>.</span><span>.</span>"));
        assert!(!body.contains("GhostCom Protocol"));
        assert!(!body.contains("peer-to-peer chat"));
    }
}
