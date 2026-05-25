mod app;
mod install_scripts;
mod page;
mod rate_limit;
mod relay;
mod rendezvous;

use anyhow::Context;
use std::{env, net::SocketAddr};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .context("PORT must be a valid TCP port")?;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("ghostcom-site listening on http://{addr}");
    axum::serve(
        listener,
        app::app().into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
