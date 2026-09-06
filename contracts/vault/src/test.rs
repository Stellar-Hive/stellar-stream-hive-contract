#![cfg(test)]

use crate::{VaultContract, VaultContractClient};
use soroban_sdk::{
    testutils::Address as _,
    token, Address, Env,
};

struct TestCtx {
    env: Env,
    vault: VaultContractClient<'static>,
    admin: Address,
    stream_contract: Address,
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
#[should_panic]
fn test_initialize_twice_rejected() {
    let ctx = setup();
    // Re-initializing must fail (AlreadyInitialized).
    ctx.vault.initialize(&ctx.admin, &ctx.stream_contract);
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

    ctx.vault
        .release(&ctx.stream_contract, &recipient, &ctx.token_address, &200, &1);

    assert_eq!(ctx.token.balance(&recipient), 200);
    assert_eq!(ctx.vault.balance_of_stream(&1), 300);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 300);
}

#[test]
#[should_panic]
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

    ctx.vault
        .release(&attacker, &recipient, &ctx.token_address, &200, &1);
}

#[test]
#[should_panic]
fn test_release_rejects_amount_exceeding_balance() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &100, &1);

    ctx.vault
        .release(&ctx.stream_contract, &recipient, &ctx.token_address, &101, &1);
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
    ctx.vault.set_stream_contract(&ctx.admin, &new_stream_contract);
    assert_eq!(ctx.vault.get_stream_contract(), new_stream_contract);
}

#[test]
#[should_panic]
fn test_set_stream_contract_rejects_non_admin() {
    let ctx = setup();
    let attacker = Address::generate(&ctx.env);
    let new_stream_contract = Address::generate(&ctx.env);
    ctx.vault.set_stream_contract(&attacker, &new_stream_contract);
}

#[test]
fn test_old_stream_contract_loses_access_after_repoint() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &500, &1);

    let new_stream_contract = Address::generate(&ctx.env);
    ctx.vault.set_stream_contract(&ctx.admin, &new_stream_contract);

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
    ctx.vault
        .release(&new_stream_contract, &recipient, &ctx.token_address, &100, &1);
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

    ctx.vault
        .release(&ctx.stream_contract, &depositor, &ctx.token_address, &100, &1);
    assert_eq!(ctx.vault.total_locked(&ctx.token_address), 400);
}

#[test]
#[should_panic]
fn test_deposit_rejects_token_mismatch_for_existing_stream() {
    let ctx = setup();
    let depositor = Address::generate(&ctx.env);
    ctx.token_sac.mint(&depositor, &1_000);
    ctx.vault.deposit(&depositor, &ctx.token_address, &100, &1);

    let other_admin = Address::generate(&ctx.env);
    let other_sac = ctx.env.register_stellar_asset_contract_v2(other_admin.clone());
    let other_token = other_sac.address();
    let other_client = token::StellarAssetClient::new(&ctx.env, &other_token);
    other_client.mint(&depositor, &1_000);

    // Same stream_id, different token: must be rejected.
    ctx.vault.deposit(&depositor, &other_token, &50, &1);
}

#[test]
fn test_balance_of_unknown_stream_is_zero() {
    let ctx = setup();
    assert_eq!(ctx.vault.balance_of_stream(&999), 0);
}
