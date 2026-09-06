use soroban_sdk::{contracttype, Address};

/// Mirrors `StreamStatus` from the stream contract. Kept as an independent
/// definition (rather than a crate dependency) so the registry stays fully
/// decoupled and can be deployed, upgraded, or replaced without touching
/// the stream contract's build graph.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamStatus {
    Pending,
    Active,
    Completed,
    Cancelled,
}

/// What the registry knows about one stream. This is a summary record for
/// indexing/analytics purposes — the stream contract remains the source of
/// truth for a stream's authoritative state.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamRecord {
    pub stream_id: u64,
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    pub amount: i128,
    pub withdrawn: i128,
    pub status: StreamStatus,
}

/// Aggregate, protocol-wide statistics maintained incrementally as streams
/// are registered and updated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryStats {
    pub total_streams: u64,
    pub active_streams: u64,
    /// Cumulative `amount` committed across every stream ever registered
    /// (a lifetime "total value streamed" metric, not a live balance).
    pub total_streamed: i128,
    /// Cumulative amount withdrawn across all streams, as last reported by
    /// `update_stream`.
    pub total_withdrawn: i128,
}
