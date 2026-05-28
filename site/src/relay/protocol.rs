use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Create {
        access_token: Option<String>,
        device_auth: Option<DeviceAuth>,
    },
    GroupCreate {
        access_token: Option<String>,
        device_auth: Option<DeviceAuth>,
    },
    Join {
        code: String,
        access_token: Option<String>,
        device_auth: Option<DeviceAuth>,
    },
    GroupJoin {
        code: String,
        access_token: Option<String>,
        device_auth: Option<DeviceAuth>,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceAuth {
    pub public_key: String,
    pub nonce: String,
    pub signature: String,
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
    DeviceApprovalRequired {
        public_key: String,
        fingerprint: String,
        suggested_approval: String,
        expires_at: u64,
    },
    Error { message: String },
}

impl Drop for ServerMessage {
    fn drop(&mut self) {
        match self {
            Self::Created { code } | Self::GroupCreated { code } => code.zeroize(),
            Self::DeviceApprovalRequired {
                public_key,
                fingerprint,
                suggested_approval,
                ..
            } => {
                public_key.zeroize();
                fingerprint.zeroize();
                suggested_approval.zeroize();
            }
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

pub fn device_auth_payload(action: &str, code: Option<&str>, nonce: &str) -> String {
    format!(
        "GhostCom relay device auth v1\n{action}\n{}\n{nonce}",
        code.unwrap_or("")
    )
}
