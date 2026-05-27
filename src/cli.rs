use anyhow::{Result, bail};
use std::io::{self, Write};
use std::net::SocketAddr;

const DEFAULT_RELAY_URL: &str = "wss://ghostcom-site.fly.dev/relay";

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
        return interactive_menu();
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
            let mut rendezvous = None;

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
                        rendezvous = Some(value);
                    }
                    other => bail!("unknown call option: {other}"),
                }
            }

            let Some(rendezvous) = rendezvous else {
                bail!(
                    "call requires --rendezvous for advanced direct setup; use relay-call for the default hosted flow"
                );
            };

            Ok(Command::Call { bind, rendezvous })
        }
        "join" => {
            let Some(code) = args.next() else {
                print_usage();
                bail!("missing invite code");
            };
            let mut rendezvous = None;

            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--rendezvous" | "--rv" => {
                        let Some(value) = args.next() else {
                            bail!("{arg} requires a WebSocket URL");
                        };
                        rendezvous = Some(value);
                    }
                    other => bail!("unknown join option: {other}"),
                }
            }

            let Some(rendezvous) = rendezvous else {
                bail!(
                    "join requires --rendezvous for advanced direct setup; use relay-join for the default hosted flow"
                );
            };

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
        "GhostCom\n\nUsage:\n  ghstprtcl\n  ghstprtcl relay-call [--relay {DEFAULT_RELAY_URL}]\n  ghstprtcl relay-join <invite-code> [--relay {DEFAULT_RELAY_URL}]\n  ghstprtcl listen [--bind 0.0.0.0:7777]\n  ghstprtcl connect <host>:7777\n\nAdvanced direct rendezvous:\n  ghstprtcl call --rendezvous wss://your-private-site/rv [--bind 0.0.0.0:7777]\n  ghstprtcl join <invite-code> --rendezvous wss://your-private-site/rv"
    );
}

fn interactive_menu() -> Result<Command> {
    println!("GhostCom");
    println!();
    println!("1. Start secure chat");
    println!("2. Join secure chat");
    println!("3. Listen directly");
    println!("4. Connect directly");
    println!();

    match prompt("Choose [1]: ")?.trim() {
        "" | "1" => Ok(Command::RelayCall {
            relay: DEFAULT_RELAY_URL.to_string(),
        }),
        "2" => {
            let code = prompt("Invite code: ")?;
            let code = code.trim().to_string();
            if code.is_empty() {
                bail!("invite code is required");
            }
            Ok(Command::RelayJoin {
                code,
                relay: DEFAULT_RELAY_URL.to_string(),
            })
        }
        "3" => Ok(Command::Listen {
            bind: "0.0.0.0:7777".parse()?,
        }),
        "4" => {
            let target = prompt("Peer address, for example 192.168.1.20:7777: ")?;
            let target = target.trim().to_string();
            if target.is_empty() {
                bail!("peer address is required");
            }
            Ok(Command::Connect { target })
        }
        other => bail!("unknown menu option: {other}"),
    }
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input)
}
