//! Wire observations are immutable and distinct from editable intent.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginalCapture(pub Vec<u8>);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedMessage {
    pub method: String,
    pub target: String,
    /// Ordered, case-preserving names and byte-valued headers.
    pub headers: Vec<(String, Vec<u8>)>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditedMessage {
    pub revision: u64,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireExecution {
    pub original: OriginalCapture,
    pub parsed: ParsedMessage,
    pub outgoing: Vec<u8>,
    pub response: Vec<u8>,
    #[serde(default)]
    pub response_outgoing: Vec<u8>,
    #[serde(default)]
    pub protocol: String,
    pub response_total_bytes: u64,
    pub response_status: u16,
    pub response_truncated: bool,
    #[serde(default)]
    pub elapsed_ms: f64,
    #[serde(default)]
    pub first_byte_ms: f64,
    #[serde(default)]
    pub held_ms: f64,
    #[serde(default)]
    pub response_mime: String,
    pub transformations: Vec<String>,
    #[serde(default)]
    pub run: Option<Box<crate::jobs::Record>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    Listening(std::net::SocketAddr),
    /// Counts decrypted application bytes; TLS record bytes are not captured.
    InterceptedTls {
        authority: String,
        uploaded: u64,
        downloaded: u64,
    },
    Exchange(WireExecution),
    Tunnel {
        authority: String,
        uploaded: u64,
        downloaded: u64,
    },
    WebSocket {
        connection: u64,
        target: String,
        direction: String,
        kind: String,
        payload: Vec<u8>,
        total: usize,
        truncated: bool,
        closed: bool,
    },
    Error(String),
}
