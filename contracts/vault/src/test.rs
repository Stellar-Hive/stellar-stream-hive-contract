#![cfg(test)]

use crate::{VaultContract, VaultContractClient, VaultError};
use soroban_sdk::{testutils::Address as _, token, Address, Env};

struct TestCtx {
    env: Env,
    vault: VaultContractClient<'static>,
    admin: Address,
    stream_contract: Address,
    #[allow(dead_code)]
    token_admin: Address,
    token_address: Address,
    token_sac: token::StellarAssetClient<'static>,
    token: token::Client<'static>,
}

fn setup() -> TestCtx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let stream_contract = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = sac.address();
    let token_sac = token::StellarAssetClient::new(&env, &token_address);
    let token = token::Client::new(&env, &token_address);

    let vault_id = env.register(VaultContract, ());
    let vault = VaultContractClient::new(&env, &vault_id);
    vault.initialize(&admin, &stream_contract);

    TestCtx {
        env,
        vault,
        admin,
        stream_contract,
        token_admin,
        token_address,
        token_sac,
        token,
    }
}

#[test]
fn test_initialize_sets_admin_and_stream_contract() {
    let ctx = setup();
    assert_eq!(ctx.vault.get_admin(), ctx.admin);
    assert_eq!(ctx.vault.get_stream_contract(), ctx.stream_contract);
}

#[test]
fn test_initialize_twice_rejected() {
    let ctx = setup();
    // Re-initializing must fail with the typed error, not with any panic.
    let result = ctx.vault.try_initialize(&ctx.admin, &ctx.stream_contract);
    assert_eq!(result, Err(Ok(VaultError::AlreadyInitialized)));
}

#[test]
fn test_deposit_transfers_tokens_into_vault() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);

    ctx.vault.deposit(&depositor, &ctx.token_address, &400, &1);

    assert_eq!(ctx.token.balance(&depositor), 600);
    assert_eq!(ctx.token.balance(&ctx.vault.address), 400);
    assert_eq!(ctx.vault.balance_of_stream(&1), 400);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 400);
}

#[test]
fn test_deposit_accumulates_across_multiple_calls() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);

    ctx.vault.deposit(&depositor, &ctx.token_address, &200, &7);
    ctx.vault.deposit(&depositor, &ctx.token_address, &150, &7);

    assert_eq!(ctx.vault.balance_of_stream(&7), 350);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 350);
}

#[test]
#[should_panic]
fn test_deposit_rejects_zero_amount() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    ctx.vault.deposit(&depositor, &ctx.token_address, &0, &1);
}

#[test]
fn test_release_by_stream_contract_succeeds() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &1);

    ctx.vault.release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &200,
        &1,
    );

    assert_eq!(ctx.token.balance(&recipient), 200);
    assert_eq!(ctx.vault.balance_of_stream(&1), 300);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 300);
}

#[test]
fn test_release_rejects_non_stream_contract_caller() {
    // SECURITY: this is the hard boundary. Anyone who is NOT the
    // configured stream contract must be rejected, even if they
    // authenticate correctly as themselves.
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let attacker = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &1);

    let result = ctx
        .vault
        .try_release(&attacker, &recipient, &ctx.token_address, &200, &1);
    assert_eq!(result, Err(Ok(VaultError::Unauthorized)));
}

#[test]
fn test_release_rejects_amount_exceeding_balance() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &1);

    let result = ctx.vault.try_release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &600,
        &1,
    );
    assert_eq!(result, Err(Ok(VaultError::InsufficientBalance)));
}

#[test]
fn test_refund_by_stream_contract_succeeds() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let sender = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &2);

    ctx.vault
        .refund(&ctx.stream_contract, &sender, &ctx.token_address, &500, &2);

    assert_eq!(ctx.token.balance(&sender), 500);
    assert_eq!(ctx.vault.balance_of_stream(&2), 0);
}

#[test]
#[should_panic]
fn test_refund_rejects_non_stream_contract_caller() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let sender = Address::generate(&ctx.env);
    let attacker = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &2);

    ctx.vault
        .refund(&attacker, &sender, &ctx.token_address, &500, &2);
}

#[test]
fn test_set_stream_contract_by_admin() {
    let ctx = setup();
    let new_stream_contract = Address::generate(&ctx.env);
    ctx.vault
        .set_stream_contract(&ctx.admin, &new_stream_contract);
    assert_eq!(ctx.vault.get_stream_contract(), new_stream_contract);
}

#[test]
#[should_panic]
fn test_set_stream_contract_rejects_non_admin() {
    let ctx = setup();
    let attacker = Address::generate(&ctx.env);
    let new_stream_contract = Address::generate(&ctx.env);
    ctx.vault
        .set_stream_contract(&attacker, &new_stream_contract);
}

#[test]
fn test_old_stream_contract_loses_access_after_repoint() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &1);

    let new_stream_contract = Address::generate(&ctx.env);
    ctx.vault
        .set_stream_contract(&ctx.admin, &new_stream_contract);

    // The old stream contract address must now be rejected.
    let result = ctx.vault.try_release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &100,
        &1,
    );
    assert!(result.is_err());

    // The new one works.
    ctx.vault.release(
        &new_stream_contract,
        &recipient,
        &ctx.token_address,
        &100,
        &1,
    );
    assert_eq!(ctx.token.balance(&recipient), 100);
}

#[test]
fn test_total_locked_tracks_multiple_streams() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);

    ctx.vault.deposit(&depositor, &ctx.token_address, &300, &1);
    ctx.vault.deposit(&depositor, &ctx.token_address, &200, &2);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 500);

    ctx.vault.release(
        &ctx.stream_contract,
        &depositor,
        &ctx.token_address,
        &100,
        &1,
    );
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 400);
}

#[test]
fn test_total_locked_decreases_after_release_and_refund() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let sender = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);

    // Deposit: 500 locked for stream 1.
    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &1);
    assert_eq!(ctx.vault.balance_of_stream(&1), 500);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 500);
    assert_eq!(ctx.token.balance(&ctx.vault.address), 500);

    // Release part: 200 paid out to recipient.
    ctx.vault.release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &200,
        &1,
    );
    assert_eq!(ctx.vault.balance_of_stream(&1), 300);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 300);
    assert_eq!(ctx.token.balance(&ctx.vault.address), 300);
    assert_eq!(ctx.token.balance(&recipient), 200);

    // Refund part: 100 returned to sender.
    ctx.vault
        .refund(&ctx.stream_contract, &sender, &ctx.token_address, &100, &1);
    assert_eq!(ctx.vault.balance_of_stream(&1), 200);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 200);
    assert_eq!(ctx.token.balance(&ctx.vault.address), 200);
    assert_eq!(ctx.token.balance(&sender), 100);
}

#[test]
#[should_panic]
fn test_deposit_rejects_token_mismatch_for_existing_stream() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &100, &1);

    let other_admin = Address::generate(&ctx.env);
    let other_sac = ctx
        .env
        .register_stellar_asset_contract_v2(other_admin.clone());
    let other_token = other_sac.address();
    let other_client = token::StellarAssetClient::new(&ctx.env, &other_token);
    other_client.mint(&depositor, &1_000);

    // Same stream id, different token: must be rejected.
    ctx.vault.deposit(&depositor, &other_token, &100, &1);
}

#[test]
fn test_release_and_refund_reject_zero_and_negative_amounts() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let sender = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &1);

    let res_zero_release = ctx.vault.try_release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &0,
        &1,
    );
    assert_eq!(res_zero_release, Err(Ok(VaultError::InvalidAmount)));

    let res_neg_release = ctx.vault.try_release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &-1,
        &1,
    );
    assert_eq!(res_neg_release, Err(Ok(VaultError::InvalidAmount)));

    let res_zero_refund = ctx.vault.try_refund(
        &ctx.stream_contract,
        &sender,
        &ctx.token_address,
        &0,
        &1,
    );
    assert_eq!(res_zero_refund, Err(Ok(VaultError::InvalidAmount)));

    let res_neg_refund = ctx.vault.try_refund(
        &ctx.stream_contract,
        &sender,
        &ctx.token_address,
        &-1,
        &1,
    );
    assert_eq!(res_neg_refund, Err(Ok(VaultError::InvalidAmount)));

    assert_eq!(ctx.vault.balance_of_stream(&1), 500);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 500);
}

#[test]
fn test_refund_rejects_amount_exceeding_balance() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let sender = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &100, &1);

    let result = ctx.vault.try_refund(
        &ctx.stream_contract,
        &sender,
        &ctx.token_address,
        &101,
        &1,
    );
    assert_eq!(result, Err(Ok(VaultError::InsufficientBalance)));
    assert_eq!(ctx.vault.balance_of_stream(&1), 100);
    assert_eq!(ctx.token.balance(&sender), 0);
}

#[test]
fn test_total_locked_is_tracked_per_token() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);

    let other_admin = Address::generate(&ctx.env);
    let other_sac = ctx
        .env
        .register_stellar_asset_contract_v2(other_admin.clone());
    let other_token = other_sac.address();
    let other_sac_client = token::StellarAssetClient::new(&ctx.env, &other_token);
    other_sac_client.mint(&depositor, &2_000);

    let unused_admin = Address::generate(&ctx.env);
    let unused_token = ctx
        .env
        .register_stellar_asset_contract_v2(unused_admin)
        .address();

    ctx.vault.deposit(&depositor, &ctx.token_address, &300, &1);
    ctx.vault.deposit(&depositor, &other_token, &700, &2);

    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 300);
    assert_eq!(ctx.vault.total_locked(&other_token), 700);
    assert_eq!(ctx.vault.total_locked(&unused_token), 0);

    ctx.vault.release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &100,
        &1,
    );
    ctx.vault
        .refund(&ctx.stream_contract, &depositor, &other_token, &250, &2);

    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 200);
    assert_eq!(ctx.vault.total_locked(&other_token), 450);
    assert_eq!(ctx.vault.total_locked(&unused_token), 0);
}

#[test]
fn test_deposit_release_and_refund_emit_events() {
    use crate::events;
    use soroban_sdk::{testutils::Events as _, Event as _};

    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let sender = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);

    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &1);
    let deposit_events = ctx.env.events().all();
    assert_eq!(
        deposit_events.filter_by_contract(&ctx.vault.address),
        [events::Deposit {
            stream_id: 1,
            from: depositor.clone(),
            token: ctx.token_address.clone(),
            amount: 500,
        }
        .to_xdr(&ctx.env, &ctx.vault.address)],
    );

    ctx.vault.release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &200,
        &1,
    );
    let release_events = ctx.env.events().all();
    assert_eq!(
        release_events.filter_by_contract(&ctx.vault.address),
        [events::Release {
            stream_id: 1,
            to: recipient.clone(),
            token: ctx.token_address.clone(),
            amount: 200,
        }
        .to_xdr(&ctx.env, &ctx.vault.address)],
    );

    ctx.vault
        .refund(&ctx.stream_contract, &sender, &ctx.token_address, &300, &1);
    let refund_events = ctx.env.events().all();
    assert_eq!(
        refund_events.filter_by_contract(&ctx.vault.address),
        [events::Refund {
            stream_id: 1,
            to: sender.clone(),
            token: ctx.token_address.clone(),
            amount: 300,
        }
        .to_xdr(&ctx.env, &ctx.vault.address)],
    );
}

#[test]
fn test_refund_with_wrong_token_returns_token_mismatch() {
    use soroban_sdk::testutils::Events as _;

    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let sender = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &1);

    let other_admin = Address::generate(&ctx.env);
    let other_sac = ctx
        .env
        .register_stellar_asset_contract_v2(other_admin.clone());
    let other_token = other_sac.address();
    let other_token_client = token::Client::new(&ctx.env, &other_token);

    let result = ctx.vault.try_refund(
        &ctx.stream_contract,
        &sender,
        &other_token,
        &100,
        &1,
    );
    assert_eq!(result, Err(Ok(VaultError::TokenMismatch)));
    assert!(ctx.env.events().all().events().is_empty());

    assert_eq!(ctx.token.balance(&depositor), 500);
    assert_eq!(ctx.token.balance(&sender), 0);
    assert_eq!(ctx.token.balance(&ctx.vault.address), 500);
    assert_eq!(other_token_client.balance(&sender), 0);
    assert_eq!(ctx.vault.balance_of_stream(&1), 500);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 500);
    assert_eq!(ctx.vault.total_locked(&other_token), 0);
}

#[test]
fn test_release_before_initialize_returns_not_initialized() {
    use soroban_sdk::testutils::Events as _;

    let env = Env::default();
    env.mock_all_auths();

    let caller = Address::generate(&env);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = sac.address();
    let token_sac = token::StellarAssetClient::new(&env, &token_address);
    let token_client = token::Client::new(&env, &token_address);
    token_sac.mint(&depositor, &1_000);

    let vault_id = env.register(VaultContract, ());
    let vault = VaultContractClient::new(&env, &vault_id);

    vault.deposit(&depositor, &token_address, &400, &1);

    let result = vault.try_release(&caller, &recipient, &token_address, &100, &1);
    assert_eq!(result, Err(Ok(VaultError::NotInitialized)));
    assert!(env.events().all().events().is_empty());

    assert_eq!(token_client.balance(&depositor), 600);
    assert_eq!(token_client.balance(&recipient), 0);
    assert_eq!(token_client.balance(&vault.address), 400);
    assert_eq!(vault.balance_of_stream(&1), 400);
    assert_eq!(vault.total_locked(&token_address), 400);
}

#[test]
fn test_set_stream_contract_emits_event_with_new_address() {
    use crate::events;
    use soroban_sdk::{testutils::Events as _, Event as _};

    let ctx = setup();
    let new_stream_contract = Address::generate(&ctx.env);

    ctx.vault
        .set_stream_contract(&ctx.admin, &new_stream_contract);

    let emitted = ctx.env.events().all();
    assert_eq!(
        emitted.filter_by_contract(&ctx.vault.address),
        [events::SetStreamContract {
            new_contract: new_stream_contract.clone(),
        }
        .to_xdr(&ctx.env, &ctx.vault.address)],
    );
    assert_eq!(ctx.vault.get_stream_contract(), new_stream_contract);
}

#[test]
fn test_deposits_from_different_senders_for_same_stream_accumulate() {
    use crate::events;
    use soroban_sdk::{testutils::Events as _, Event as _};

    let ctx = setup();
    let sender_a = Address::generate(&ctx.env);
    let sender_b = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.token_sac.mint(&sender_a, &1_000);
    ctx.token_sac.mint(&sender_b, &1_000);

    ctx.vault.deposit(&sender_a, &ctx.token_address, &350, &42);
    let events_a = ctx.env.events().all();
    assert_eq!(
        events_a.filter_by_contract(&ctx.vault.address),
        [events::Deposit {
            stream_id: 42,
            from: sender_a.clone(),
            token: ctx.token_address.clone(),
            amount: 350,
        }
        .to_xdr(&ctx.env, &ctx.vault.address)],
    );

    ctx.vault.deposit(&sender_b, &ctx.token_address, &250, &42);
    let events_b = ctx.env.events().all();
    assert_eq!(
        events_b.filter_by_contract(&ctx.vault.address),
        [events::Deposit {
            stream_id: 42,
            from: sender_b.clone(),
            token: ctx.token_address.clone(),
            amount: 250,
        }
        .to_xdr(&ctx.env, &ctx.vault.address)],
    );

    assert_eq!(ctx.vault.balance_of_stream(&42), 600);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 600);
    assert_eq!(ctx.token.balance(&sender_a), 650);
    assert_eq!(ctx.token.balance(&sender_b), 750);
    assert_eq!(ctx.token.balance(&ctx.vault.address), 600);

    ctx.vault.release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &400,
        &42,
    );
    let release_events = ctx.env.events().all();
    assert_eq!(
        release_events.filter_by_contract(&ctx.vault.address),
        [events::Release {
            stream_id: 42,
            to: recipient.clone(),
            token: ctx.token_address.clone(),
            amount: 400,
        }
        .to_xdr(&ctx.env, &ctx.vault.address)],
    );

    ctx.vault.refund(
        &ctx.stream_contract,
        &sender_a,
        &ctx.token_address,
        &200,
        &42,
    );
    let refund_events = ctx.env.events().all();
    assert_eq!(
        refund_events.filter_by_contract(&ctx.vault.address),
        [events::Refund {
            stream_id: 42,
            to: sender_a.clone(),
            token: ctx.token_address.clone(),
            amount: 200,
        }
        .to_xdr(&ctx.env, &ctx.vault.address)],
    );

    assert_eq!(ctx.vault.balance_of_stream(&42), 0);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 0);
    assert_eq!(ctx.token.balance(&recipient), 400);
    assert_eq!(ctx.token.balance(&sender_a), 850);
    assert_eq!(ctx.token.balance(&sender_b), 750);
    assert_eq!(ctx.token.balance(&ctx.vault.address), 0);
}

#[test]
fn test_deposit_rejects_negative_amount() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);

    let result = ctx
        .vault
        .try_deposit(&depositor, &ctx.token_address, &-1, &1);
    assert_eq!(result, Err(Ok(VaultError::InvalidAmount)));
    assert_eq!(ctx.vault.balance_of_stream(&1), 0);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 0);
    assert_eq!(ctx.token.balance(&ctx.vault.address), 0);
    assert_eq!(ctx.token.balance(&depositor), 1_000);
}

#[test]
fn test_set_stream_contract_before_initialize_returns_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let new_contract = Address::generate(&env);
    let vault_id = env.register(VaultContract, ());
    let vault = VaultContractClient::new(&env, &vault_id);

    let result = vault.try_set_stream_contract(&admin, &new_contract);
    assert_eq!(result, Err(Ok(VaultError::NotInitialized)));
}

#[test]
fn test_get_admin_and_get_stream_contract_fail_before_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let vault_id = env.register(VaultContract, ());
    let vault = VaultContractClient::new(&env, &vault_id);

    assert_eq!(vault.try_get_admin(), Err(Ok(VaultError::NotInitialized)));
    let stream_contract = vault.try_get_stream_contract();
    assert_eq!(stream_contract, Err(Ok(VaultError::NotInitialized)));
}

#[test]
fn test_releasing_full_balance_leaves_zero_locked() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);

    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &1);
    assert_eq!(ctx.vault.balance_of_stream(&1), 500);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 500);

    ctx.vault.release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &200,
        &1,
    );
    ctx.vault.release(
        &ctx.stream_contract,
        &recipient,
        &ctx.token_address,
        &300,
        &1,
    );

    assert_eq!(ctx.vault.balance_of_stream(&1), 0);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 0);
    assert_eq!(ctx.token.balance(&ctx.vault.address), 0);
    assert_eq!(ctx.token.balance(&recipient), 500);
}
