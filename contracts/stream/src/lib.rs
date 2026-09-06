//! # Stellar Stream Hive — Stream Contract
//!
//! The core of the system: linear, per-ledger token streaming. A stream
//! moves `total_amount` of a token from `sender` to `recipient` evenly
//! between `start_ledger` and `end_ledger`. At any point the recipient can
//! withdraw whatever has accrued so far (after `cliff_ledger`) and not yet
//! been withdrawn.
//!
//! This contract never custodies funds itself — that's the vault's job
//! (see `stellar-stream-hive-vault`). This contract only tracks stream
//! metadata and instructs the vault to move money via `deposit` /
//! `release` / `refund`.
#![no_std]

mod error;
mod events;
mod math;
mod storage;
mod test;
mod types;
mod vault_client;

use error::StreamError;
use soroban_sdk::{contract, contractimpl, Address, Env, Vec};
use types::{Stream, StreamStatus};

#[contract]
pub struct StreamContract;

#[contractimpl]
impl StreamContract {
    /// One-time setup. Records the admin and the vault that will custody
    /// every stream created through this contract.
    pub fn initialize(env: Env, admin: Address, vault: Address) -> Result<(), StreamError> {
        if storage::has_admin(&env) {
            return Err(StreamError::AlreadyInitialized);
        }
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_vault(&env, &vault);
        Ok(())
    }

    /// Creates a new stream and pulls `total_amount` of `token` from
    /// `sender` into the vault. Returns the new stream's id.
    pub fn create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        total_amount: i128,
        start_ledger: u32,
        end_ledger: u32,
        cliff_ledger: u32,
        cancellable: bool,
    ) -> Result<u64, StreamError> {
        sender.require_auth();

        if total_amount <= 0 {
            return Err(StreamError::InvalidAmount);
        }
        if end_ledger <= start_ledger {
            return Err(StreamError::InvalidTimeRange);
        }
        if cliff_ledger < start_ledger || cliff_ledger > end_ledger {
            return Err(StreamError::InvalidCliff);
        }

        let vault = storage::get_vault(&env).ok_or(StreamError::NotInitialized)?;
        let id = storage::next_stream_id(&env);
        let now = env.ledger().sequence();

        let stream = Stream {
            id,
            sender: sender.clone(),
            recipient: recipient.clone(),
            token: token.clone(),
            total_amount,
            withdrawn: 0,
            start_ledger,
            end_ledger,
            cliff_ledger,
            cancellable,
            status: StreamStatus::Pending,
            created_at: now,
            cancelled_at_ledger: 0,
        };
        storage::set_stream(&env, &stream);
        storage::add_sender_stream(&env, &sender, id);
        storage::add_recipient_stream(&env, &recipient, id);

        vault_client::deposit(&env, &vault, &sender, &token, total_amount, id);

        events::Create {
            stream_id: id,
            sender,
            recipient,
            token,
            total_amount,
            start_ledger,
            end_ledger,
        }
        .publish(&env);

        Ok(id)
    }

    /// Total value accrued so far, ignoring withdrawals. See
    /// [`math::streamed_amount`] for the exact formula.
    pub fn streamed_amount(env: Env, stream_id: u64) -> Result<i128, StreamError> {
        let stream = storage::get_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        let now = env.ledger().sequence();
        math::streamed_amount(&stream, now)
    }

    /// `streamed_amount - withdrawn`, or 0 before the cliff.
    pub fn withdrawable_amount(env: Env, stream_id: u64) -> Result<i128, StreamError> {
        let stream = storage::get_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        let now = env.ledger().sequence();
        math::withdrawable_amount(&stream, now)
    }

    /// The recipient withdraws `amount` (up to `withdrawable_amount`) from
    /// their stream.
    pub fn withdraw(env: Env, recipient: Address, stream_id: u64, amount: i128) -> Result<(), StreamError> {
        recipient.require_auth();
        if amount <= 0 {
            return Err(StreamError::InvalidAmount);
        }

        let mut stream = storage::get_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.recipient != recipient {
            return Err(StreamError::Unauthorized);
        }

        let now = env.ledger().sequence();
        let available = math::withdrawable_amount(&stream, now)?;
        if amount > available {
            return Err(StreamError::ExceedsWithdrawable);
        }

        stream.withdrawn = stream
            .withdrawn
            .checked_add(amount)
            .ok_or(StreamError::MathOverflow)?;
        storage::set_stream(&env, &stream);

        let vault = storage::get_vault(&env).ok_or(StreamError::NotInitialized)?;
        vault_client::release(
            &env,
            &vault,
            &env.current_contract_address(),
            &recipient,
            &stream.token,
            amount,
            stream_id,
        );

        events::Withdraw {
            stream_id,
            recipient,
            amount,
        }
        .publish(&env);
        Ok(())
    }

    /// Convenience: withdraws the entire currently-withdrawable balance.
    pub fn withdraw_max(env: Env, recipient: Address, stream_id: u64) -> Result<i128, StreamError> {
        recipient.require_auth();
        let stream = storage::get_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.recipient != recipient {
            return Err(StreamError::Unauthorized);
        }
        let now = env.ledger().sequence();
        let available = math::withdrawable_amount(&stream, now)?;
        if available == 0 {
            return Ok(0);
        }
        Self::withdraw(env, recipient, stream_id, available)?;
        Ok(available)
    }

    /// The sender cancels a cancellable stream. The recipient keeps
    /// whatever has streamed so far (frozen at this ledger); the sender is
    /// refunded the remainder.
    pub fn cancel_stream(env: Env, sender: Address, stream_id: u64) -> Result<(), StreamError> {
        sender.require_auth();

        let mut stream = storage::get_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        if stream.sender != sender {
            return Err(StreamError::Unauthorized);
        }
        if !stream.cancellable {
            return Err(StreamError::NotCancellable);
        }

        let now = env.ledger().sequence();
        let live_status = math::compute_status(&stream, now);
        if live_status == StreamStatus::Cancelled || live_status == StreamStatus::Completed {
            return Err(StreamError::AlreadyFinalized);
        }

        let streamed_now = math::streamed_amount(&stream, now)?;
        let remainder = stream
            .total_amount
            .checked_sub(streamed_now)
            .ok_or(StreamError::MathOverflow)?;

        stream.status = StreamStatus::Cancelled;
        stream.cancelled_at_ledger = now;
        storage::set_stream(&env, &stream);

        if remainder > 0 {
            let vault = storage::get_vault(&env).ok_or(StreamError::NotInitialized)?;
            vault_client::refund(
                &env,
                &vault,
                &env.current_contract_address(),
                &sender,
                &stream.token,
                remainder,
                stream_id,
            );
        }

        events::Cancel {
            stream_id,
            sender,
            streamed_now,
            remainder,
        }
        .publish(&env);
        Ok(())
    }

    /// Returns the stream with its `status` freshly recomputed for the
    /// current ledger.
    pub fn get_stream(env: Env, stream_id: u64) -> Result<Stream, StreamError> {
        let mut stream = storage::get_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        let now = env.ledger().sequence();
        stream.status = math::compute_status(&stream, now);
        Ok(stream)
    }

    /// All streams where `sender` is the sender, freshest status first
    /// isn't guaranteed — insertion order is preserved.
    pub fn get_streams_by_sender(env: Env, sender: Address) -> Vec<Stream> {
        let ids = storage::get_sender_streams(&env, &sender);
        Self::hydrate(&env, &ids)
    }

    /// All streams where `recipient` is the recipient.
    pub fn get_streams_by_recipient(env: Env, recipient: Address) -> Vec<Stream> {
        let ids = storage::get_recipient_streams(&env, &recipient);
        Self::hydrate(&env, &ids)
    }

    /// The live, derived status of a stream.
    pub fn get_status(env: Env, stream_id: u64) -> Result<StreamStatus, StreamError> {
        let stream = storage::get_stream(&env, stream_id).ok_or(StreamError::StreamNotFound)?;
        let now = env.ledger().sequence();
        Ok(math::compute_status(&stream, now))
    }
}

impl StreamContract {
    fn hydrate(env: &Env, ids: &Vec<u64>) -> Vec<Stream> {
        let now = env.ledger().sequence();
        let mut out = Vec::new(env);
        for id in ids.iter() {
            if let Some(mut s) = storage::get_stream(env, id) {
                s.status = math::compute_status(&s, now);
                out.push_back(s);
            }
        }
        out
    }
}

// Re-exported so integration tests in sibling crates (and downstream
// tooling) can name these types via `stellar_stream_hive_stream::...`.
pub use error::StreamError as Error;
pub use types::{Stream as StreamRecord, StreamStatus as Status};
