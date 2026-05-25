use crate::protocol::frame::{Frame, read_frame, validate_display_name, write_frame};
use anyhow::Result;
use std::io::Write;
use tokio::io::{AsyncRead, AsyncWrite};

pub async fn confirm_peer(verification_code: &str) -> Result<bool> {
    println!();
    println!("Session verification code:");
    println!("  {verification_code}");
    println!();
    println!("This code must exactly match on both terminals.");
    print!("Type YES to start chatting, or anything else to disconnect: ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(is_confirmation(input.trim()))
}

fn is_confirmation(input: &str) -> bool {
    input.eq_ignore_ascii_case("yes") || input.eq_ignore_ascii_case("y")
}

pub fn prompt_display_name(default_name: &str) -> Result<String> {
    println!();
    print!("Display name for this session [{default_name}]: ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let name = input.trim();

    let selected = if name.is_empty() {
        default_name.to_string()
    } else {
        name.to_string()
    };
    validate_display_name(&selected)?;
    Ok(selected)
}

pub async fn run_chat<S>(stream: S, peer_name: String) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (reader, writer) = tokio::io::split(stream);
    let mut peer_reader = reader;
    let mut peer_writer = writer;
    let mut stdin_lines = spawn_stdin_reader();

    println!("Chat started with {peer_name}. Type /quit to close the session.");

    loop {
        tokio::select! {
            line = stdin_lines.recv() => {
                let Some(line) = line else {
                    write_frame(&mut peer_writer, Frame::Close).await?;
                    break;
                };

                if line.trim() == "/quit" {
                    write_frame(&mut peer_writer, Frame::Close).await?;
                    break;
                }

                write_frame(&mut peer_writer, Frame::Chat(line)).await?;
            }
            frame = read_frame(&mut peer_reader) => {
                match frame? {
                    Frame::Hello(_) => {}
                    Frame::Chat(message) => println!("{peer_name}> {message}"),
                    Frame::Close => {
                        println!("Peer closed the session.");
                        break;
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                let _ = write_frame(&mut peer_writer, Frame::Close).await;
                break;
            }
        }
    }

    Ok(())
}

pub(crate) fn spawn_stdin_reader() -> tokio::sync::mpsc::UnboundedReceiver<String> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    std::thread::spawn(move || {
        loop {
            let mut line = String::new();
            match std::io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    trim_line_endings(&mut line);
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            }
        }
    });

    receiver
}

fn trim_line_endings(line: &mut String) {
    while line.ends_with(['\n', '\r']) {
        line.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::is_confirmation;

    #[test]
    fn accepts_common_confirmation_inputs() {
        assert!(is_confirmation("YES"));
        assert!(is_confirmation("yes"));
        assert!(is_confirmation("y"));
        assert!(is_confirmation("Y"));
        assert!(!is_confirmation(""));
        assert!(!is_confirmation("no"));
    }
}
