use soroban_sdk::contracterror;

/// All error conditions the vault contract can return. Using an explicit
/// error enum (rather than `panic!`) means callers — and tests — get a
/// stable, typed reason for every rejected call instead of an opaque trap.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum VaultError {
    /// `initialize` was called a second time.
    AlreadyInitialized = 1,
    /// A function that requires `initialize` to have run first was called
    /// before it was.
    NotInitialized = 2,
    /// The caller authenticated successfully but is not the address
    /// authorized to perform this action (e.g. not the configured stream
    /// contract, or not the admin).
    Unauthorized = 3,
    /// An amount argument was zero or negative.
    InvalidAmount = 4,
    /// A stream's locked balance is smaller than the amount requested to
    /// move out of it.
    InsufficientBalance = 5,
    /// Checked arithmetic overflowed or underflowed.
    MathOverflow = 6,
    /// The token passed for a stream does not match the token it was
    /// originally funded with.
    TokenMismatch = 7,
}
