use crate::install_scripts::{INSTALL_PS1, INSTALL_SH};
use crate::page::INDEX_HTML;
use crate::relay::{RelayState, relay_ws};
use crate::rendezvous::{RendezvousState, rendezvous_ws};
use axum::{
    Router,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};

pub fn app() -> Router {
    let public_routes = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/install.sh", get(install_sh))
        .route("/install.ps1", get(install_ps1))
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

async fn install_sh() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        INSTALL_SH,
    )
        .into_response()
}

async fn install_ps1() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        INSTALL_PS1,
    )
        .into_response()
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

    #[tokio::test]
    async fn install_scripts_are_available_but_not_on_index() {
        let sh_response = install_sh().await;
        let sh_body = to_bytes(sh_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let sh_body = String::from_utf8(sh_body.to_vec()).unwrap();

        assert!(sh_body.contains("GHSTPRTCL_REPO"));
        assert!(sh_body.contains("SHA256SUMS"));

        let ps_response = install_ps1().await;
        let ps_body = to_bytes(ps_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let ps_body = String::from_utf8(ps_body.to_vec()).unwrap();

        assert!(ps_body.contains("GHSTPRTCL_REPO"));
        assert!(ps_body.contains("Get-FileHash"));
    }
}
