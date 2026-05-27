use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Create {
        access_token: Option<String>,
    },
    Join {
        code: String,
        access_token: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Created { code: String },
    Joined,
    PeerJoined,
    Error { message: String },
}
