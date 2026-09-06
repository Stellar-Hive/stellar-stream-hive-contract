//! # Stellar Stream Hive — Vault Contract
//!
//! The vault is the sole custodian of tokens locked into payment streams.
//! It never makes streaming-math decisions itself — it just holds funds and
//! moves them when instructed to by the stream contract.
//!
//! ## Security boundary
//! [`release`] and [`refund`] are the only ways funds leave the vault, and
//! both require the caller to (a) cryptographically authenticate as the
//! address they claim to be (`caller.require_auth()`) *and* (b) match the
//! single `stream_contract` address configured at [`initialize`] /
//! [`set_stream_contract`]. Anyone else calling these functions is rejected
//! with [`VaultError::Unauthorized`] before any token movement happens.
#![no_std]

mod error;
mod events;
mod storage;
mod test;

use error::VaultError;
use soroban_sdk::{contract, contractimpl, token, Address, Env};

#[contract]
pub struct VaultContract;

#[contractimpl]
impl VaultContract {
    /// One-time setup. Records the admin (who may later repoint the
    /// authorized stream contract) and the stream contract allowed to move
    /// funds held by this vault.
    pub fn initialize(env: Env, admin: Address, stream_contract: Address) -> Result<(), VaultError> {
        if storage::has_admin(&env) {
            return Err(VaultError::AlreadyInitialized);
        }
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_stream_contract(&env, &stream_contract);
        Ok(())
    }

    /// Pulls `amount` of `token` from `from` into the vault and credits it
    /// to `stream_id`'s locked balance. Called by the stream contract as
    /// part of `create_stream`.
    pub fn deposit(
        env: Env,
        from: Address,
        token: Address,
        amount: i128,
        stream_id: u64,
    ) -> Result<(), VaultError> {
        if amount <= 0 {
            return Err(VaultError::InvalidAmount);
        }
        from.require_auth();

        // Defensive consistency check: a stream_id must always be funded
        // with a single, fixed token for its entire lifetime.
        if let Some(existing_token) = storage::get_stream_token(&env, stream_id) {
            if existing_token != token {
                return Err(VaultError::TokenMismatch);
            }
        } else {
            storage::set_stream_token(&env, stream_id, &token);
        }

        let client = token::Client::new(&env, &token);
        client.transfer(&from, &env.current_contract_address(), &amount);

        let new_balance = storage::get_stream_balance(&env, stream_id)
            .checked_add(amount)
            .ok_or(VaultError::MathOverflow)?;
        storage::set_stream_balance(&env, stream_id, new_balance);

        let new_total = storage::get_total_locked(&env, &token)
            .checked_add(amount)
            .ok_or(VaultError::MathOverflow)?;
        storage::set_total_locked(&env, &token, new_total);

        events::Deposit {
            stream_id,
            from,
            token,
            amount,
        }
        .publish(&env);
        Ok(())
    }

    /// Releases `amount` of `token` from `stream_id`'s locked balance to
    /// `to`. This is the withdrawal path, invoked by the stream contract
    /// when a recipient withdraws their accrued balance.
    ///
    /// SECURITY: `caller` must authenticate AND must equal the configured
    /// `stream_contract`. This is the only thing standing between the
    /// vault's funds and anyone who can construct a call.
    pub fn release(
        env: Env,
        caller: Address,
        to: Address,
        token: Address,
        amount: i128,
        stream_id: u64,
    ) -> Result<(), VaultError> {
        Self::authorize_and_pay(&env, caller, to, token, amount, stream_id, PayoutKind::Release)
    }

    /// Refunds `amount` of `token` from `stream_id`'s locked balance to
    /// `to` (the original sender). Invoked by the stream contract when a
    /// cancellable stream is cancelled and unstreamed funds must return to
    /// the sender.
    ///
    /// SECURITY: identical caller check to [`release`].
    pub fn refund(
        env: Env,
        caller: Address,
        to: Address,
        token: Address,
        amount: i128,
        stream_id: u64,
    ) -> Result<(), VaultError> {
        Self::authorize_and_pay(&env, caller, to, token, amount, stream_id, PayoutKind::Refund)
    }

    /// Locked balance currently attributed to `stream_id`.
    pub fn balance_of_stream(env: Env, stream_id: u64) -> i128 {
        storage::get_stream_balance(&env, stream_id)
    }

    /// Total amount of `token` locked in the vault across all streams.
    pub fn total_locked(env: Env, token: Address) -> i128 {
        storage::get_total_locked(&env, &token)
    }

    /// Admin-only: repoints the single contract address authorized to call
    /// `release` / `refund`. Useful for upgrading the stream contract
    /// without redeploying the vault (and re-migrating custodied funds).
    pub fn set_stream_contract(env: Env, admin: Address, new_contract: Address) -> Result<(), VaultError> {
        let stored_admin = storage::get_admin(&env).ok_or(VaultError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(VaultError::Unauthorized);
        }
        storage::set_stream_contract(&env, &new_contract);
        events::SetStreamContract { new_contract }.publish(&env);
        Ok(())
    }

    /// Read-only accessor for the currently configured stream contract.
    pub fn get_stream_contract(env: Env) -> Result<Address, VaultError> {
        storage::get_stream_contract(&env).ok_or(VaultError::NotInitialized)
    }

    /// Read-only accessor for the admin address.
    pub fn get_admin(env: Env) -> Result<Address, VaultError> {
        storage::get_admin(&env).ok_or(VaultError::NotInitialized)
    }
}

enum PayoutKind {
    Release,
    Refund,
}

impl VaultContract {
    fn authorize_and_pay(
        env: &Env,
        caller: Address,
        to: Address,
        token: Address,
        amount: i128,
        stream_id: u64,
        kind: PayoutKind,
    ) -> Result<(), VaultError> {
        if amount <= 0 {
            return Err(VaultError::InvalidAmount);
        }

        let stream_contract = storage::get_stream_contract(env).ok_or(VaultError::NotInitialized)?;

        // 1. The caller must cryptographically prove they are who they say.
        caller.require_auth();
        // 2. ...and that identity must be the single trusted stream contract.
        if caller != stream_contract {
            return Err(VaultError::Unauthorized);
        }

        if let Some(existing_token) = storage::get_stream_token(env, stream_id) {
            if existing_token != token {
                return Err(VaultError::TokenMismatch);
            }
        }

        let current_balance = storage::get_stream_balance(env, stream_id);
        if current_balance < amount {
            return Err(VaultError::InsufficientBalance);
        }

        let new_balance = current_balance
            .checked_sub(amount)
            .ok_or(VaultError::MathOverflow)?;
        storage::set_stream_balance(env, stream_id, new_balance);

        let current_total = storage::get_total_locked(env, &token);
        let new_total = current_total
            .checked_sub(amount)
            .ok_or(VaultError::MathOverflow)?;
        storage::set_total_locked(env, &token, new_total);

        let client = token::Client::new(env, &token);
        client.transfer(&env.current_contract_address(), &to, &amount);

        match kind {
            PayoutKind::Release => {
                events::Release {
                    stream_id,
                    to,
                    token,
                    amount,
                }
                .publish(env);
            }
            PayoutKind::Refund => {
                events::Refund {
                    stream_id,
                    to,
                    token,
                    amount,
                }
                .publish(env);
            }
        }
        Ok(())
    }
}

pub use error::VaultError as Error;
pub use storage::DataKey as VaultDataKey;
