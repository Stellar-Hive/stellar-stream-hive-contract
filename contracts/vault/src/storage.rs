use soroban_sdk::{contracttype, Address, Env};

/// Ledgers a persistent storage entry survives without being touched
/// before it is eligible for archival. Kept generous since streams (and
/// their vault balances) can legitimately live for a long time.
const LEDGER_THRESHOLD: u32 = 120_960; // ~7 days at 5s ledgers
const LEDGER_BUMP: u32 = 241_920; // ~14 days at 5s ledgers

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    StreamContract,
    /// Locked balance currently attributed to a given stream id.
    StreamBalance(u64),
    /// The token a given stream id was funded with (set on first deposit).
    StreamToken(u64),
    /// Running total of a token's balance locked in the vault across all
    /// streams.
    TotalLocked(Address),
}

pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
    env.storage()
        .instance()
        .extend_ttl(LEDGER_THRESHOLD, LEDGER_BUMP);
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_stream_contract(env: &Env, addr: &Address) {
    env.storage().instance().set(&DataKey::StreamContract, addr);
    env.storage()
        .instance()
        .extend_ttl(LEDGER_THRESHOLD, LEDGER_BUMP);
}

pub fn get_stream_contract(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::StreamContract)
}

pub fn get_stream_balance(env: &Env, stream_id: u64) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::StreamBalance(stream_id))
        .unwrap_or(0)
}

pub fn set_stream_balance(env: &Env, stream_id: u64, amount: i128) {
    let key = DataKey::StreamBalance(stream_id);
    env.storage().persistent().set(&key, &amount);
    env.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP);
}

pub fn get_stream_token(env: &Env, stream_id: u64) -> Option<Address> {
    env.storage().persistent().get(&DataKey::StreamToken(stream_id))
}

pub fn set_stream_token(env: &Env, stream_id: u64, token: &Address) {
    let key = DataKey::StreamToken(stream_id);
    env.storage().persistent().set(&key, token);
    env.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP);
}

pub fn get_total_locked(env: &Env, token: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::TotalLocked(token.clone()))
        .unwrap_or(0)
}

pub fn set_total_locked(env: &Env, token: &Address, amount: i128) {
    let key = DataKey::TotalLocked(token.clone());
    env.storage().persistent().set(&key, &amount);
    env.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP);
}
