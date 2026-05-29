use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Create { access_token: Option<String> },
    GroupCreate { access_token: Option<String> },
    Join { code: String, access_token: Option<String> },
    GroupJoin { code: String, access_token: Option<String> },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Created { code: String },
    GroupCreated { code: String },
    Joined,
    GroupJoined,
    PeerJoined,
    GroupPeerJoined { peer_id: u16 },
    GroupPeerLeft { peer_id: u16 },
    Error { message: String },
}

impl Drop for ServerMessage {
    fn drop(&mut self) {
        match self {
            Self::Created { code } | Self::GroupCreated { code } => code.zeroize(),
            Self::Error { message } => message.zeroize(),
            Self::Joined
            | Self::GroupJoined
            | Self::PeerJoined
            | Self::GroupPeerJoined { .. }
            | Self::GroupPeerLeft { .. } => {}
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostControlMessage {
    CloseGroupInvite,
}
