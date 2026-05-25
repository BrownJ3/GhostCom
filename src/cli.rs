use anyhow::{Result, bail};
use std::net::SocketAddr;

const DEFAULT_RENDEZVOUS_URL: &str = "ws://127.0.0.1:8080/rv";
const DEFAULT_RELAY_URL: &str = "ws://127.0.0.1:8080/relay";

pub enum Command {
    RelayCall {
        relay: String,
    },
    RelayJoin {
        code: String,
        relay: String,
    },
    Call {
        bind: SocketAddr,
        rendezvous: String,
    },
    Join {
        code: String,
        rendezvous: String,
    },
    Listen {
        bind: SocketAddr,
    },
    Connect {
        target: String,
    },
}

pub fn parse() -> Result<Command> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage();
        bail!("missing command");
    };

    match command.as_str() {
        "relay-call" => {
            let mut relay = DEFAULT_RELAY_URL.to_string();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--relay" => {
                        let Some(value) = args.next() else {
                            bail!("--relay requires a WebSocket URL");
                        };
                        relay = value;
                    }
                    other => bail!("unknown relay-call option: {other}"),
                }
            }
            Ok(Command::RelayCall { relay })
        }
        "relay-join" => {
            let Some(code) = args.next() else {
                print_usage();
                bail!("missing relay invite code");
            };
            let mut relay = DEFAULT_RELAY_URL.to_string();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--relay" => {
                        let Some(value) = args.next() else {
                            bail!("--relay requires a WebSocket URL");
                        };
                        relay = value;
                    }
                    other => bail!("unknown relay-join option: {other}"),
                }
            }
            Ok(Command::RelayJoin { code, relay })
        }
        "call" => {
            let mut bind = "0.0.0.0:7777".parse()?;
            let mut rendezvous = DEFAULT_RENDEZVOUS_URL.to_string();

            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--bind" => {
                        let Some(value) = args.next() else {
                            bail!("--bind requires an address");
                        };
                        bind = value.parse()?;
                    }
                    "--rendezvous" | "--rv" => {
                        let Some(value) = args.next() else {
                            bail!("{arg} requires a WebSocket URL");
                        };
                        rendezvous = value;
                    }
                    other => bail!("unknown call option: {other}"),
                }
            }

            Ok(Command::Call { bind, rendezvous })
        }
        "join" => {
            let Some(code) = args.next() else {
                print_usage();
                bail!("missing invite code");
            };
            let mut rendezvous = DEFAULT_RENDEZVOUS_URL.to_string();

            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--rendezvous" | "--rv" => {
                        let Some(value) = args.next() else {
                            bail!("{arg} requires a WebSocket URL");
                        };
                        rendezvous = value;
                    }
                    other => bail!("unknown join option: {other}"),
                }
            }

            Ok(Command::Join { code, rendezvous })
        }
        "listen" => {
            let bind = match args.next().as_deref() {
                Some("--bind") => args
                    .next()
                    .unwrap_or_else(|| "0.0.0.0:7777".to_string())
                    .parse()?,
                Some(addr) => addr.parse()?,
                None => "0.0.0.0:7777".parse()?,
            };
            Ok(Command::Listen { bind })
        }
        "connect" => {
            let Some(target) = args.next() else {
                print_usage();
                bail!("missing target");
            };
            Ok(Command::Connect { target })
        }
        "-h" | "--help" | "help" => {
            print_usage();
            std::process::exit(0);
        }
        other => {
            print_usage();
            bail!("unknown command: {other}");
        }
    }
}

fn print_usage() {
    eprintln!(
        "GhostCom\n\nUsage:\n  ghostcom relay-call [--relay ws://127.0.0.1:8080/relay]\n  ghostcom relay-join <invite-code> [--relay ws://127.0.0.1:8080/relay]\n  ghostcom call [--bind 0.0.0.0:7777] [--rendezvous ws://127.0.0.1:8080/rv]\n  ghostcom join <invite-code> [--rendezvous ws://127.0.0.1:8080/rv]\n  ghostcom listen [--bind 0.0.0.0:7777]\n  ghostcom connect <host>:7777"
    );
}
