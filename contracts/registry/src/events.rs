//! Typed contract events, defined with `#[contractevent]` rather than
//! published as untyped tuples.
use soroban_sdk::contractevent;

#[contractevent(data_format = "single-value")]
pub struct Register {
    #[topic]
    pub stream_id: u64,
    pub amount: i128,
}

#[contractevent(data_format = "single-value")]
pub struct Update {
    #[topic]
    pub stream_id: u64,
    pub withdrawn: i128,
}
