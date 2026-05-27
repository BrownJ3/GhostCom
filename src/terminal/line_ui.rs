use crate::protocol::frame::{Frame, read_frame, validate_display_name, write_frame};
use anyhow::Result;
use crossterm::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers, poll, read},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use std::io::Write;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use zeroize::Zeroize;

pub async fn confirm_peer(verification_code: &str) -> Result<bool> {
    println!();
    println!("Session verification");
    println!("--------------------------------------------------");
    println!("{verification_code}");
    println!("--------------------------------------------------");
    println!();
    println!("This code must exactly match on both terminals.");
    print!("Type YES to start chatting, or anything else to disconnect: ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let confirmed = is_confirmation(input.trim());
    input.zeroize();
    Ok(confirmed)
}

fn is_confirmation(input: &str) -> bool {
    input.eq_ignore_ascii_case("yes") || input.eq_ignore_ascii_case("y")
}

pub fn prompt_display_name(default_name: &str) -> Result<String> {
    println!();
    print!("Display name [{default_name}]: ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let name = input.trim();

    let selected = if name.is_empty() {
        default_name.to_string()
    } else {
        name.to_string()
    };
    input.zeroize();
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
    let mut input_events = ChatInputReader::spawn();
    let mut typing_indicator = TypingIndicator::new(peer_name.clone());
    let typing_enabled = typing_enabled();
    let mut tick = tokio::time::interval(Duration::from_millis(350));

    chat_println(&format!(
        "Chat started with {peer_name}. Type /quit to close the session."
    ))?;
    chat_prompt()?;

    loop {
        tokio::select! {
            input = input_events.recv() => {
                let Some(input) = input else {
                    write_frame(&mut peer_writer, Frame::Close).await?;
                    break;
                };

                match input {
                    ChatInput::Line(mut line) => {
                        if line.trim() == "/quit" {
                            line.zeroize();
                            write_frame(&mut peer_writer, Frame::Close).await?;
                            break;
                        }

                        write_frame(&mut peer_writer, Frame::Chat(line)).await?;
                    }
                    ChatInput::TypingStart => {
                        if typing_enabled {
                            write_frame(&mut peer_writer, Frame::TypingStart).await?;
                        }
                    }
                    ChatInput::TypingStop => {
                        if typing_enabled {
                            write_frame(&mut peer_writer, Frame::TypingStop).await?;
                        }
                    }
                    ChatInput::Closed => {
                        write_frame(&mut peer_writer, Frame::Close).await?;
                        break;
                    }
                }
            }
            frame = read_frame(&mut peer_reader) => {
                match frame? {
                    Frame::Hello(_) => {}
                    Frame::Chat(mut message) => {
                        typing_indicator.stop()?;
                        chat_println(&format!(
                            "{peer_name}> {}",
                            sanitize_for_terminal(&message)
                        ))?;
                        message.zeroize();
                        chat_prompt()?;
                    }
                    Frame::TypingStart => typing_indicator.start()?,
                    Frame::TypingStop => typing_indicator.stop()?,
                    Frame::Close => {
                        typing_indicator.stop()?;
                        chat_println("Peer closed the session.")?;
                        break;
                    }
                }
            }
            _ = tick.tick() => {
                typing_indicator.tick()?;
            }
            _ = tokio::signal::ctrl_c() => {
                let _ = write_frame(&mut peer_writer, Frame::Close).await;
                break;
            }
        }
    }

    Ok(())
}

pub(crate) fn typing_enabled() -> bool {
    std::env::var("GHSTPRTCL_ENABLE_TYPING")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ChatInput {
    Line(String),
    TypingStart,
    TypingStop,
    Closed,
}

pub(crate) struct ChatInputReader {
    receiver: tokio::sync::mpsc::UnboundedReceiver<ChatInput>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ChatInputReader {
    pub(crate) fn spawn() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);

        let thread = std::thread::spawn(move || {
            let _raw_mode = RawModeGuard::enable();
            let mut line = String::new();
            let mut typing = false;

            loop {
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                if !poll(Duration::from_millis(50)).unwrap_or(false) {
                    continue;
                }
                let Ok(Event::Key(key)) = read() else {
                    continue;
                };
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        line.zeroize();
                        let _ = sender.send(ChatInput::Closed);
                        break;
                    }
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        line.zeroize();
                        let _ = sender.send(ChatInput::Closed);
                        break;
                    }
                    KeyCode::Char(ch) => {
                        line.push(ch);
                        print!("{ch}");
                        let _ = std::io::stdout().flush();
                        if !typing {
                            typing = true;
                            if sender.send(ChatInput::TypingStart).is_err() {
                                break;
                            }
                        }
                    }
                    KeyCode::Backspace | KeyCode::Delete => {
                        if line.pop().is_some() {
                            print!("\x08 \x08");
                            let _ = std::io::stdout().flush();
                        }
                        if line.is_empty() && typing {
                            typing = false;
                            if sender.send(ChatInput::TypingStop).is_err() {
                                break;
                            }
                        }
                    }
                    KeyCode::Enter => {
                        let _ = chat_println("");
                        if typing {
                            typing = false;
                            if sender.send(ChatInput::TypingStop).is_err() {
                                break;
                            }
                        }
                        let submitted = std::mem::take(&mut line);
                        if sender.send(ChatInput::Line(submitted)).is_err() {
                            break;
                        }
                        let _ = chat_prompt();
                    }
                    _ => {}
                }
            }
        });

        Self {
            receiver,
            stop,
            thread: Some(thread),
        }
    }

    pub(crate) async fn recv(&mut self) -> Option<ChatInput> {
        self.receiver.recv().await
    }
}

impl Drop for ChatInputReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> Option<Self> {
        enable_raw_mode().ok()?;
        Some(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

pub(crate) struct TypingIndicator {
    peer_name: String,
    active: bool,
    dots: usize,
}

impl TypingIndicator {
    pub(crate) fn new(peer_name: String) -> Self {
        Self {
            peer_name,
            active: false,
            dots: 0,
        }
    }

    pub(crate) fn start(&mut self) -> Result<()> {
        self.active = true;
        self.dots = 0;
        self.render()
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        if self.active {
            self.active = false;
            clear_current_line()?;
            chat_prompt()?;
        }
        Ok(())
    }

    pub(crate) fn tick(&mut self) -> Result<()> {
        if self.active {
            self.dots = (self.dots + 1) % 4;
            self.render()?;
        }
        Ok(())
    }

    fn render(&self) -> Result<()> {
        let dots = ".".repeat(self.dots);
        print!(
            "\r\x1b[2K{} is typing{dots:<3}",
            sanitize_for_terminal(&self.peer_name)
        );
        std::io::stdout().flush()?;
        Ok(())
    }
}

pub(crate) fn chat_prompt() -> Result<()> {
    print!("you> ");
    std::io::stdout().flush()?;
    Ok(())
}

pub(crate) fn chat_println(line: &str) -> Result<()> {
    print!("{line}\r\n");
    std::io::stdout().flush()?;
    Ok(())
}

pub(crate) fn chat_status(line: &str) -> Result<()> {
    chat_println(&format!("[status] {line}"))
}

pub(crate) fn chat_success(line: &str) -> Result<()> {
    chat_println(&format!("[secure] {line}"))
}

pub(crate) fn print_invite_box(label: &str, code: &str) -> Result<()> {
    println!();
    println!("{label}");
    println!("--------------------------------------------------");
    println!("{code}");
    println!("--------------------------------------------------");
    println!();
    std::io::stdout().flush()?;
    Ok(())
}

fn clear_current_line() -> Result<()> {
    print!("\r\x1b[2K");
    std::io::stdout().flush()?;
    Ok(())
}

pub(crate) fn sanitize_for_terminal(text: &str) -> String {
    let mut sanitized = String::with_capacity(text.len());

    for ch in text.chars() {
        if is_terminal_unsafe(ch) {
            sanitized.push_str(&ch.escape_unicode().to_string());
        } else {
            sanitized.push(ch);
        }
    }

    sanitized
}

fn is_terminal_unsafe(ch: char) -> bool {
    ch.is_control()
        || matches!(
            ch,
            '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

#[cfg(test)]
mod tests {
    use super::{is_confirmation, sanitize_for_terminal};

    #[test]
    fn accepts_common_confirmation_inputs() {
        assert!(is_confirmation("YES"));
        assert!(is_confirmation("yes"));
        assert!(is_confirmation("y"));
        assert!(is_confirmation("Y"));
        assert!(!is_confirmation(""));
        assert!(!is_confirmation("no"));
    }

    #[test]
    fn sanitizes_terminal_controls() {
        assert_eq!(
            sanitize_for_terminal("hi\u{1b}[2J\nthere"),
            "hi\\u{1b}[2J\\u{a}there"
        );
        assert_eq!(sanitize_for_terminal("ab\u{202e}cd"), "ab\\u{202e}cd");
    }
}
