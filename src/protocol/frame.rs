use anyhow::{Result, bail};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAGIC: &[u8; 2] = b"GC";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 8;
const MAX_FRAME_PAYLOAD: usize = 16 * 1024;
const MAX_CHAT_PAYLOAD: usize = 8 * 1024;
const MAX_NAME_PAYLOAD: usize = 64;

#[derive(Debug, Eq, PartialEq)]
pub enum Frame {
    Hello(String),
    Chat(String),
    Close,
}

#[derive(Clone, Copy)]
enum FrameType {
    Hello = 1,
    Chat = 2,
    Close = 3,
}

impl FrameType {
    fn from_byte(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Hello),
            2 => Ok(Self::Chat),
            3 => Ok(Self::Close),
            _ => bail!("unknown frame type"),
        }
    }
}

pub async fn write_frame<W>(writer: &mut W, frame: Frame) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let (frame_type, payload) = match frame {
        Frame::Hello(name) => {
            validate_display_name(&name)?;
            (FrameType::Hello, name.into_bytes())
        }
        Frame::Chat(message) => {
            let bytes = message.into_bytes();
            if bytes.len() > MAX_CHAT_PAYLOAD {
                bail!("message too large");
            }
            (FrameType::Chat, bytes)
        }
        Frame::Close => (FrameType::Close, Vec::new()),
    };

    if payload.len() > MAX_FRAME_PAYLOAD {
        bail!("frame too large");
    }

    let mut header = [0_u8; HEADER_LEN];
    header[0..2].copy_from_slice(MAGIC);
    header[2] = VERSION;
    header[3] = frame_type as u8;
    header[4..8].copy_from_slice(&(payload.len() as u32).to_be_bytes());

    writer.write_all(&header).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_frame<R>(reader: &mut R) -> Result<Frame>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0_u8; HEADER_LEN];
    reader.read_exact(&mut header).await?;

    if &header[0..2] != MAGIC {
        bail!("invalid frame magic");
    }
    if header[2] != VERSION {
        bail!("unsupported protocol version");
    }

    let frame_type = FrameType::from_byte(header[3])?;
    let payload_len = u32::from_be_bytes(header[4..8].try_into()?) as usize;
    if payload_len > MAX_FRAME_PAYLOAD {
        bail!("frame too large");
    }

    let mut payload = vec![0_u8; payload_len];
    reader.read_exact(&mut payload).await?;

    match frame_type {
        FrameType::Hello => {
            if payload.len() > MAX_NAME_PAYLOAD {
                bail!("display name too large");
            }
            let name = String::from_utf8(payload)?;
            validate_display_name(&name)?;
            Ok(Frame::Hello(name))
        }
        FrameType::Chat => {
            if payload.len() > MAX_CHAT_PAYLOAD {
                bail!("chat message too large");
            }
            Ok(Frame::Chat(String::from_utf8(payload)?))
        }
        FrameType::Close => {
            if !payload.is_empty() {
                bail!("close frame must be empty");
            }
            Ok(Frame::Close)
        }
    }
}

pub fn validate_display_name(name: &str) -> Result<()> {
    if name.trim() != name {
        bail!("display name must not have leading or trailing spaces");
    }
    if name.is_empty() {
        bail!("display name must not be empty");
    }
    if name.len() > MAX_NAME_PAYLOAD {
        bail!("display name too large");
    }
    if name
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '<' | '>' | '\r' | '\n' | '\t'))
    {
        bail!("display name contains unsupported characters");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn round_trips_chat_frame() {
        let (mut client, mut server) = duplex(1024);

        write_frame(&mut client, Frame::Chat("hello".to_string()))
            .await
            .unwrap();

        let frame = read_frame(&mut server).await.unwrap();
        assert_eq!(frame, Frame::Chat("hello".to_string()));
    }

    #[tokio::test]
    async fn round_trips_hello_name() {
        let (mut client, mut server) = duplex(1024);

        write_frame(&mut client, Frame::Hello("NovaSignal1234".to_string()))
            .await
            .unwrap();

        let frame = read_frame(&mut server).await.unwrap();
        assert_eq!(frame, Frame::Hello("NovaSignal1234".to_string()));
    }

    #[tokio::test]
    async fn rejects_unknown_frame_type() {
        let (mut client, mut server) = duplex(1024);
        client
            .write_all(&[b'G', b'C', VERSION, 99, 0, 0, 0, 0])
            .await
            .unwrap();

        assert!(read_frame(&mut server).await.is_err());
    }

    #[test]
    fn rejects_bad_display_names() {
        assert!(validate_display_name("Alice").is_ok());
        assert!(validate_display_name("").is_err());
        assert!(validate_display_name(" Alice").is_err());
        assert!(validate_display_name("Alice\n").is_err());
        assert!(validate_display_name("Alice\t").is_err());
    }
}
