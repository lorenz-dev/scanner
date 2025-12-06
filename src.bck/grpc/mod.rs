pub mod multi_grpc;

#[derive(Clone)]
pub struct ConnectionContext {
    pub endpoint: String,
    pub status: ConnectionStatus,
    pub ping: u64,
}

#[derive(Clone)]
pub enum ConnectionStatus {
    Connected,
    Connecting,
    Disconnected,
}
