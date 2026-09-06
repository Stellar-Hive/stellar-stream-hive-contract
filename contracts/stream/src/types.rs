use soroban_sdk::{contracttype, Address};

/// Lifecycle state of a stream. `Pending`/`Active`/`Completed` are always
/// derived from the current ledger sequence versus `start_ledger` /
/// `end_ledger` — they are never persisted as-is. `Cancelled` is the one
/// terminal state that a sender can force early, and it IS persisted
/// (recorded in `Stream.status`) because it can't be derived from time
/// alone.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamStatus {
    /// Created, but `start_ledger` has not been reached yet.
    Pending,
    /// Between `start_ledger` and `end_ledger`; actively accruing.
    Active,
    /// `end_ledger` has been reached or passed; fully streamed.
    Completed,
    /// Cancelled early by the sender before completion.
    Cancelled,
}

/// A single payment stream.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stream {
    pub id: u64,
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    pub total_amount: i128,
    pub withdrawn: i128,
    pub start_ledger: u32,
    pub end_ledger: u32,
    pub cliff_ledger: u32,
    pub cancellable: bool,
    pub status: StreamStatus,
    pub created_at: u32,
    /// Ledger sequence at which the stream was cancelled. `0` if it has
    /// never been cancelled. Used to freeze `streamed_amount` at the exact
    /// value it held the moment of cancellation, so ledger progress after
    /// cancellation can never inflate what the recipient is owed.
    pub cancelled_at_ledger: u32,
}
