mod cli;
mod protocol;
mod relay;
mod rendezvous;
mod security;
mod terminal;
mod transport;

use anyhow::Result;
use cli::Command;

#[tokio::main]
async fn main() -> Result<()> {
    match cli::parse()? {
        Command::RelayCall { relay } => relay::call(relay).await,
        Command::RelayGroup { relay } => relay::group(relay).await,
        Command::RelayJoin { code, relay } => relay::join(code, relay).await,
        Command::Call { bind, rendezvous } => transport::call(bind, rendezvous).await,
        Command::Join { code, rendezvous } => transport::join(code, rendezvous).await,
        Command::Listen { bind } => transport::listen(bind).await,
        Command::Connect { target } => transport::connect(target).await,
    }
}
