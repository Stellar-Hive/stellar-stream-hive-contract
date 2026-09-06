use soroban_sdk::{contracttype, vec, Env, Vec};

use crate::types::{RegistryStats, StreamRecord};

const LEDGER_THRESHOLD: u32 = 120_960; // ~7 days at 5s ledgers
const LEDGER_BUMP: u32 = 241_920; // ~14 days at 5s ledgers

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    AllStreamIds,
    Record(u64),
    Stats,
}

pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &soroban_sdk::Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
    env.storage()
        .instance()
        .extend_ttl(LEDGER_THRESHOLD, LEDGER_BUMP);
}

pub fn get_admin(env: &Env) -> Option<soroban_sdk::Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn get_all_stream_ids(env: &Env) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&DataKey::AllStreamIds)
        .unwrap_or_else(|| vec![env])
}

pub fn add_stream_id(env: &Env, stream_id: u64) {
    let mut ids = get_all_stream_ids(env);
    ids.push_back(stream_id);
    env.storage().persistent().set(&DataKey::AllStreamIds, &ids);
    env.storage()
        .persistent()
        .extend_ttl(&DataKey::AllStreamIds, LEDGER_THRESHOLD, LEDGER_BUMP);
}

pub fn get_record(env: &Env, stream_id: u64) -> Option<StreamRecord> {
    env.storage().persistent().get(&DataKey::Record(stream_id))
}

pub fn set_record(env: &Env, record: &StreamRecord) {
    let key = DataKey::Record(record.stream_id);
    env.storage().persistent().set(&key, record);
    env.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP);
}

pub fn get_stats(env: &Env) -> RegistryStats {
    env.storage().instance().get(&DataKey::Stats).unwrap_or(RegistryStats {
        total_streams: 0,
        active_streams: 0,
        total_streamed: 0,
        total_withdrawn: 0,
    })
}

pub fn set_stats(env: &Env, stats: &RegistryStats) {
    env.storage().instance().set(&DataKey::Stats, stats);
    env.storage()
        .instance()
        .extend_ttl(LEDGER_THRESHOLD, LEDGER_BUMP);
}
