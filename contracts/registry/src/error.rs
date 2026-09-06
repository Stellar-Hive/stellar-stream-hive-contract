use soroban_sdk::contracterror;

/// All error conditions the registry contract can return.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RegistryError {
    /// `initialize` was called a second time.
    AlreadyInitialized = 1,
    /// A function that requires `initialize` to have run first was called
    /// before it was.
    NotInitialized = 2,
    /// An amount argument was negative.
    InvalidAmount = 3,
    /// `register_stream` was called twice with the same `stream_id`.
    AlreadyRegistered = 4,
    /// `update_stream` referenced a `stream_id` that was never registered.
    StreamNotFound = 5,
    /// Checked arithmetic overflowed or underflowed.
    MathOverflow = 6,
}
