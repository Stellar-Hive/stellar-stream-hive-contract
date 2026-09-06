//! Typed contract events, defined with `#[contractevent]` so they are
//! included in the contract's interface specification (and so tooling /
//! generated clients can decode them) rather than published as untyped
//! tuples via the deprecated `Events::publish`.
use soroban_sdk::{contractevent, Address};

#[contractevent(data_format = "vec")]
pub struct Deposit {
    #[topic]
    pub stream_id: u64,
    pub from: Address,
    pub token: Address,
    pub amount: i128,
}

#[contractevent(data_format = "vec")]
pub struct Release {
    #[topic]
    pub stream_id: u64,
    pub to: Address,
    pub token: Address,
    pub amount: i128,
}

#[contractevent(data_format = "vec")]
pub struct Refund {
    #[topic]
    pub stream_id: u64,
    pub to: Address,
    pub token: Address,
    pub amount: i128,
}

#[contractevent(data_format = "single-value")]
pub struct SetStreamContract {
    pub new_contract: Address,
}
