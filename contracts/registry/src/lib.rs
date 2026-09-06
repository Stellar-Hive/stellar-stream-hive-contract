//! # Stellar Stream Hive — Registry Contract
//!
//! An on-chain index of every stream created across the protocol, plus
//! running aggregate statistics (total streams, active streams, total
//! volume, total withdrawn). It does not custody funds and does not gate
//! the stream/vault contracts in any way — it exists purely so that
//! dashboards, indexers, and other contracts can query protocol-wide state
//! in one place instead of enumerating every stream individually.
//!
//! ## Note on write access
//! `register_stream` and `update_stream` intentionally take no caller
//! parameter (matching the interface this contract is specified against),
//! so unlike the vault's `release`/`refund`, this contract cannot
//! cryptographically verify that only the stream contract calls them.
//! This is a deliberate, documented trade-off: the registry is an index,
//! not a custodian, so a bad-faith registration can at most corrupt
//! off-chain-visible analytics, never move funds. See `docs/registry.md`
//! for the full rationale and how to harden this further (e.g. an
//! authorized-writer allowlist) in a future version.
#![no_std]

mod error;
mod storage;
mod types;
mod test;

use error::RegistryError;
use soroban_sdk::{contract, contractimpl, Address, Env, Vec};
pub use types::{RegistryStats, StreamRecord, StreamStatus};

#[contract]
pub struct RegistryContract;

#[contractimpl]
impl RegistryContract {
    /// One-time setup.
    pub fn initialize(env: Env, admin: Address) -> Result<(), RegistryError> {
        if storage::has_admin(&env) {
            return Err(RegistryError::AlreadyInitialized);
        }
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_stats(
            &env,
            &RegistryStats {
                total_streams: 0,
                active_streams: 0,
                total_streamed: 0,
                total_withdrawn: 0,
            },
        );
        Ok(())
    }

    /// Registers a newly created stream. New streams are recorded as
    /// `Active` (streams are indexed right after creation, and any
    /// pre-cliff/pre-start nuance is left to the stream contract itself —
    /// `update_stream` can correct the status later).
    pub fn register_stream(
        env: Env,
        stream_id: u64,
        sender: Address,
        recipient: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), RegistryError> {
        if amount < 0 {
            return Err(RegistryError::InvalidAmount);
        }
        if storage::get_record(&env, stream_id).is_some() {
            return Err(RegistryError::AlreadyRegistered);
        }

        let record = StreamRecord {
            stream_id,
            sender,
            recipient,
            token,
            amount,
            withdrawn: 0,
            status: StreamStatus::Active,
        };
        storage::set_record(&env, &record);
        storage::add_stream_id(&env, stream_id);

        let mut stats = storage::get_stats(&env);
        stats.total_streams = stats
            .total_streams
            .checked_add(1)
            .ok_or(RegistryError::MathOverflow)?;
        stats.active_streams = stats
            .active_streams
            .checked_add(1)
            .ok_or(RegistryError::MathOverflow)?;
        stats.total_streamed = stats
            .total_streamed
            .checked_add(amount)
            .ok_or(RegistryError::MathOverflow)?;
        storage::set_stats(&env, &stats);

        env.events()
            .publish((soroban_sdk::symbol_short!("register"), stream_id), amount);
        Ok(())
    }

    /// Updates a previously registered stream's withdrawn amount and
    /// status, keeping aggregate stats consistent.
    pub fn update_stream(
        env: Env,
        stream_id: u64,
        withdrawn: i128,
        status: StreamStatus,
    ) -> Result<(), RegistryError> {
        if withdrawn < 0 {
            return Err(RegistryError::InvalidAmount);
        }
        let mut record = storage::get_record(&env, stream_id).ok_or(RegistryError::StreamNotFound)?;

        let mut stats = storage::get_stats(&env);

        let withdrawn_delta = withdrawn
            .checked_sub(record.withdrawn)
            .ok_or(RegistryError::MathOverflow)?;
        if withdrawn_delta > 0 {
            stats.total_withdrawn = stats
                .total_withdrawn
                .checked_add(withdrawn_delta)
                .ok_or(RegistryError::MathOverflow)?;
        }

        let was_active = record.status == StreamStatus::Active;
        let will_be_active = status == StreamStatus::Active;
        if was_active && !will_be_active {
            stats.active_streams = stats.active_streams.saturating_sub(1);
        } else if !was_active && will_be_active {
            stats.active_streams = stats
                .active_streams
                .checked_add(1)
                .ok_or(RegistryError::MathOverflow)?;
        }

        record.withdrawn = withdrawn;
        record.status = status;
        storage::set_record(&env, &record);
        storage::set_stats(&env, &stats);

        env.events()
            .publish((soroban_sdk::symbol_short!("update"), stream_id), withdrawn);
        Ok(())
    }

    /// All registered stream ids, in registration order.
    pub fn get_all_streams(env: Env) -> Vec<u64> {
        storage::get_all_stream_ids(&env)
    }

    /// Protocol-wide aggregate statistics.
    pub fn get_stats(env: Env) -> RegistryStats {
        storage::get_stats(&env)
    }

    /// Ids of every stream currently recorded as `Active`.
    pub fn get_active_streams(env: Env) -> Vec<u64> {
        let ids = storage::get_all_stream_ids(&env);
        let mut out = Vec::new(&env);
        for id in ids.iter() {
            if let Some(record) = storage::get_record(&env, id) {
                if record.status == StreamStatus::Active {
                    out.push_back(id);
                }
            }
        }
        out
    }

    /// Read-only accessor for a single stream's registry record.
    pub fn get_stream_record(env: Env, stream_id: u64) -> Result<StreamRecord, RegistryError> {
        storage::get_record(&env, stream_id).ok_or(RegistryError::StreamNotFound)
    }
}

pub use error::RegistryError as Error;
