#![cfg(test)]

use crate::{StreamContract, StreamContractClient};
use crate::types::StreamStatus;
use soroban_sdk::{testutils::{Address as _, Ledger as _}, token, Address, Env};
use stellar_stream_hive_vault::{VaultContract, VaultContractClient};

struct TestCtx {
    env: Env,
    stream: StreamContractClient<'static>,
    vault: VaultContractClient<'static>,
    admin: Address,
    sender: Address,
    recipient: Address,
    token_address: Address,
    token: token::Client<'static>,
}

const START: u32 = 1_000;
const END: u32 = 2_000;
const CLIFF: u32 = 1_000;
const AMOUNT: i128 = 1_000_000;

fn setup() -> TestCtx {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(0);

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = sac.address();
    let token_sac = token::StellarAssetClient::new(&env, &token_address);
    let token = token::Client::new(&env, &token_address);
    token_sac.mint(&sender, &(AMOUNT * 10));

    let stream_id = env.register(StreamContract, ());
    let stream = StreamContractClient::new(&env, &stream_id);

    let vault_id = env.register(VaultContract, ());
    let vault = VaultContractClient::new(&env, &vault_id);

    vault.initialize(&admin, &stream_id);
    stream.initialize(&admin, &vault_id);

    TestCtx {
        env,
        stream,
        vault,
        admin,
        sender,
        recipient,
        token_address,
        token,
    }
}

fn set_ledger(ctx: &TestCtx, seq: u32) {
    ctx.env.ledger().set_sequence_number(seq);
}

fn create_default_stream(ctx: &TestCtx, cancellable: bool) -> u64 {
    ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &START,
        &END,
        &CLIFF,
        &cancellable,
    )
}

#[test]
fn test_create_stream_pulls_funds_into_vault() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    assert_eq!(id, 1);
    assert_eq!(ctx.token.balance(&ctx.vault.address), AMOUNT);
    assert_eq!(ctx.vault.balance_of_stream(&id), AMOUNT);
}

#[test]
fn test_streamed_amount_zero_before_start() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    set_ledger(&ctx, START - 1);
    assert_eq!(ctx.stream.streamed_amount(&id), 0);
}

#[test]
fn test_streamed_amount_full_after_end() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    set_ledger(&ctx, END + 500);
    assert_eq!(ctx.stream.streamed_amount(&id), AMOUNT);
}

#[test]
fn test_streamed_amount_half_at_midpoint() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);
    assert_eq!(ctx.stream.streamed_amount(&id), AMOUNT / 2);
}

#[test]
fn test_withdrawable_zero_before_cliff() {
    let ctx = setup();
    // cliff well after start so streaming has begun but cliff hasn't hit.
    let id = ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &START,
        &END,
        &(START + 400),
        &true,
    );
    set_ledger(&ctx, START + 200);
    assert!(ctx.stream.streamed_amount(&id) > 0);
    assert_eq!(ctx.stream.withdrawable_amount(&id), 0);
}

#[test]
fn test_withdraw_reduces_withdrawable() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);

    let before = ctx.stream.withdrawable_amount(&id);
    assert_eq!(before, AMOUNT / 2);

    ctx.stream.withdraw(&ctx.recipient, &id, &(AMOUNT / 4));

    let after = ctx.stream.withdrawable_amount(&id);
    assert_eq!(after, AMOUNT / 2 - AMOUNT / 4);
    assert_eq!(ctx.token.balance(&ctx.recipient), AMOUNT / 4);
}

#[test]
#[should_panic]
fn test_withdraw_rejects_amount_exceeding_withdrawable() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);
    ctx.stream.withdraw(&ctx.recipient, &id, &(AMOUNT));
}

#[test]
#[should_panic]
fn test_withdraw_rejects_non_recipient_caller() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    set_ledger(&ctx, END);
    let stranger = Address::generate(&ctx.env);
    ctx.stream.withdraw(&stranger, &id, &1);
}

#[test]
fn test_withdraw_max_pulls_entire_withdrawable_balance() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    set_ledger(&ctx, END);
    let amount = ctx.stream.withdraw_max(&ctx.recipient, &id);
    assert_eq!(amount, AMOUNT);
    assert_eq!(ctx.token.balance(&ctx.recipient), AMOUNT);
    assert_eq!(ctx.stream.withdrawable_amount(&id), 0);
}

#[test]
fn test_cancel_splits_funds_between_recipient_and_sender() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);

    ctx.stream.cancel_stream(&ctx.sender, &id);

    // Sender gets refunded the unstreamed half immediately.
    assert_eq!(ctx.token.balance(&ctx.sender), AMOUNT * 9 + AMOUNT / 2);
    // Recipient can still withdraw the streamed half, frozen at cancel time.
    assert_eq!(ctx.stream.withdrawable_amount(&id), AMOUNT / 2);
    ctx.stream.withdraw_max(&ctx.recipient, &id);
    assert_eq!(ctx.token.balance(&ctx.recipient), AMOUNT / 2);
}

#[test]
fn test_cancel_freezes_streamed_amount_after_further_ledgers() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);
    ctx.stream.cancel_stream(&ctx.sender, &id);

    // Advance far past the original end_ledger: streamed amount must not grow.
    set_ledger(&ctx, END + 10_000);
    assert_eq!(ctx.stream.streamed_amount(&id), AMOUNT / 2);
    assert_eq!(ctx.stream.get_status(&id), StreamStatus::Cancelled);
}

#[test]
#[should_panic]
fn test_cancel_rejected_on_non_cancellable_stream() {
    let ctx = setup();
    let id = create_default_stream(&ctx, false);
    set_ledger(&ctx, START + 100);
    ctx.stream.cancel_stream(&ctx.sender, &id);
}

#[test]
#[should_panic]
fn test_cancel_rejected_for_non_sender() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    set_ledger(&ctx, START + 100);
    let stranger = Address::generate(&ctx.env);
    ctx.stream.cancel_stream(&stranger, &id);
}

#[test]
#[should_panic]
fn test_cancel_rejected_after_completion() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    set_ledger(&ctx, END + 1);
    ctx.stream.cancel_stream(&ctx.sender, &id);
}

#[test]
#[should_panic]
fn test_create_stream_rejects_invalid_time_range() {
    let ctx = setup();
    ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &END,
        &START,
        &START,
        &true,
    );
}

#[test]
#[should_panic]
fn test_create_stream_rejects_cliff_before_start() {
    let ctx = setup();
    ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &START,
        &END,
        &(START - 1),
        &true,
    );
}

#[test]
#[should_panic]
fn test_create_stream_rejects_cliff_after_end() {
    let ctx = setup();
    ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &START,
        &END,
        &(END + 1),
        &true,
    );
}

#[test]
#[should_panic]
fn test_create_stream_rejects_zero_amount() {
    let ctx = setup();
    ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &0,
        &START,
        &END,
        &START,
        &true,
    );
}

#[test]
fn test_get_status_transitions_through_lifecycle() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);

    set_ledger(&ctx, START - 1);
    assert_eq!(ctx.stream.get_status(&id), StreamStatus::Pending);

    set_ledger(&ctx, START + 1);
    assert_eq!(ctx.stream.get_status(&id), StreamStatus::Active);

    set_ledger(&ctx, END);
    assert_eq!(ctx.stream.get_status(&id), StreamStatus::Completed);
}

#[test]
fn test_get_streams_by_sender_and_recipient() {
    let ctx = setup();
    let id1 = create_default_stream(&ctx, true);
    let id2 = create_default_stream(&ctx, false);

    let by_sender = ctx.stream.get_streams_by_sender(&ctx.sender);
    assert_eq!(by_sender.len(), 2);
    assert_eq!(by_sender.get(0).unwrap().id, id1);
    assert_eq!(by_sender.get(1).unwrap().id, id2);

    let by_recipient = ctx.stream.get_streams_by_recipient(&ctx.recipient);
    assert_eq!(by_recipient.len(), 2);
}

#[test]
fn test_multiple_streams_have_independent_balances() {
    let ctx = setup();
    let id1 = create_default_stream(&ctx, true);
    let id2 = create_default_stream(&ctx, true);

    set_ledger(&ctx, END);
    ctx.stream.withdraw_max(&ctx.recipient, &id1);

    assert_eq!(ctx.stream.withdrawable_amount(&id1), 0);
    assert_eq!(ctx.stream.withdrawable_amount(&id2), AMOUNT);
}

#[test]
fn test_get_stream_returns_correct_fields() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let s = ctx.stream.get_stream(&id);
    assert_eq!(s.sender, ctx.sender);
    assert_eq!(s.recipient, ctx.recipient);
    assert_eq!(s.total_amount, AMOUNT);
    assert_eq!(s.withdrawn, 0);
}
