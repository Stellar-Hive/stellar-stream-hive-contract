//! Typed contract events, defined with `#[contractevent]` rather than
//! published as untyped tuples, so they show up in the contract's
//! interface specification.
use soroban_sdk::{contractevent, Address};

#[contractevent(data_format = "vec")]
pub struct Create {
    #[topic]
    pub stream_id: u64,
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    pub total_amount: i128,
    pub start_ledger: u32,
    pub end_ledger: u32,
}

#[contractevent(data_format = "vec")]
pub struct Withdraw {
    #[topic]
    pub stream_id: u64,
    pub recipient: Address,
    pub amount: i128,
}

#[contractevent(data_format = "vec")]
pub struct Cancel {
    #[topic]
    pub stream_id: u64,
    pub sender: Address,
    pub streamed_now: i128,
    pub remainder: i128,
}
