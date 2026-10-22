#![cfg(test)]

use crate::types::StreamStatus;
use crate::{StreamContract, StreamContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Address, Env, IntoVal,
};

/// A minimal stand-in for the real vault contract, used only so the stream
/// contract's tests can exercise the full create -> deposit -> withdraw ->
/// release -> cancel -> refund call flow through real cross-contract
/// invocation, without pulling the vault crate in as a Rust dependency.
///
/// The vault's own security boundary (caller must be the exact configured
/// stream contract) and balance accounting are exhaustively tested in
/// `stellar-stream-hive-vault`'s own test suite (see contracts/vault/src/test.rs)
/// against the real vault contract -- this mock only needs to move tokens
/// realistically enough for the stream contract's own math and control
/// flow to be tested end-to-end.
mod mock_vault {
    use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

    #[contracttype]
    enum Key {
        Balance(u64),
    }

    #[contract]
    pub struct MockVault;

    #[contractimpl]
    impl MockVault {
        pub fn deposit(env: Env, from: Address, token: Address, amount: i128, stream_id: u64) {
            from.require_auth();
            token::Client::new(&env, &token).transfer(
                &from,
                &env.current_contract_address(),
                &amount,
            );
            let key = Key::Balance(stream_id);
            let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            env.storage().persistent().set(&key, &(bal + amount));
        }

        pub fn release(
            env: Env,
            caller: Address,
            to: Address,
            token: Address,
            amount: i128,
            stream_id: u64,
        ) {
            caller.require_auth();
            let key = Key::Balance(stream_id);
            let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            env.storage().persistent().set(&key, &(bal - amount));
            token::Client::new(&env, &token).transfer(
                &env.current_contract_address(),
                &to,
                &amount,
            );
        }

        pub fn refund(
            env: Env,
            caller: Address,
            to: Address,
            token: Address,
            amount: i128,
            stream_id: u64,
        ) {
            Self::release(env, caller, to, token, amount, stream_id);
        }

        pub fn balance_of_stream(env: Env, stream_id: u64) -> i128 {
            env.storage()
                .persistent()
                .get(&Key::Balance(stream_id))
                .unwrap_or(0)
        }
    }
}

use mock_vault::{MockVault, MockVaultClient};

struct TestCtx {
    env: Env,
    stream: StreamContractClient<'static>,
    vault: MockVaultClient<'static>,
    #[allow(dead_code)]
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

    let vault_id = env.register(MockVault, ());
    let vault = MockVaultClient::new(&env, &vault_id);

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
fn test_withdraw_rejects_zero_amount() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    set_ledger(&ctx, END);

    let result = ctx.stream.try_withdraw(&ctx.recipient, &id, &0);

    assert_eq!(result, Err(Ok(StreamError::InvalidAmount)));
    assert_eq!(ctx.token.balance(&ctx.recipient), 0);
    assert_eq!(ctx.stream.withdrawable_amount(&id), AMOUNT);
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
fn test_withdraw_max_rejects_non_recipient_caller() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let stranger = Address::generate(&ctx.env);

    assert_eq!(
        ctx.stream.try_withdraw_max(&stranger, &id),
        Err(Ok(crate::error::StreamError::Unauthorized)),
    );
}

#[test]
fn test_withdraw_max_unknown_id_returns_stream_not_found() {
    let ctx = setup();

    assert_eq!(
        ctx.stream.try_withdraw_max(&ctx.recipient, &999),
        Err(Ok(crate::error::StreamError::StreamNotFound)),
    );
}

#[test]
fn test_withdrawing_full_balance_at_end_leaves_zero_locked() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    set_ledger(&ctx, END);

    ctx.stream.withdraw(&ctx.recipient, &id, &AMOUNT);

    assert_eq!(ctx.vault.balance_of_stream(&id), 0);
    assert_eq!(ctx.token.balance(&ctx.vault.address), 0);
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
fn test_cancel_after_partial_withdrawal_splits_funds() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);

    // Recipient withdraws a quarter of the accrued amount at START + 250.
    set_ledger(&ctx, START + 250);
    ctx.stream.withdraw(&ctx.recipient, &id, &(AMOUNT / 8));
    assert_eq!(ctx.token.balance(&ctx.recipient), AMOUNT / 8);

    // Cancel at the midpoint: half the total has streamed.
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);
    ctx.stream.cancel_stream(&ctx.sender, &id);

    // Sender is refunded the unstreamed half (total - streamed).
    assert_eq!(ctx.token.balance(&ctx.sender), AMOUNT * 9 + AMOUNT / 2);
    // Recipient can still claim streamed - withdrawn.
    assert_eq!(
        ctx.stream.withdrawable_amount(&id),
        AMOUNT / 2 - AMOUNT / 8
    );

    ctx.stream.withdraw_max(&ctx.recipient, &id);
    // Recipient's final balance equals the streamed amount.
    assert_eq!(ctx.token.balance(&ctx.recipient), AMOUNT / 2);
    assert_eq!(ctx.vault.balance_of_stream(&id), 0);
}

#[test]
fn test_cancel_pending_stream_returns_full_deposit() {
    let ctx = setup();
    let balance_before = ctx.token.balance(&ctx.sender);

    // START is in the future relative to ledger 0, so the stream is Pending.
    let id = create_default_stream(&ctx, true);
    assert_eq!(ctx.stream.get_status(&id), StreamStatus::Pending);

    set_ledger(&ctx, 0);
    ctx.stream.cancel_stream(&ctx.sender, &id);

    // The sender is exactly where they began.
    assert_eq!(ctx.token.balance(&ctx.sender), balance_before);
    assert_eq!(ctx.vault.balance_of_stream(&id), 0);
    assert_eq!(ctx.stream.get_status(&id), StreamStatus::Cancelled);
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
fn test_cancel_rejected_on_non_cancellable_stream() {
    let ctx = setup();
    let id = create_default_stream(&ctx, false);
    set_ledger(&ctx, START + 100);
    let result = ctx.stream.try_cancel_stream(&ctx.sender, &id);
    assert_eq!(result, Err(Ok(StreamError::NotCancellable)));
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

// ---------------------------------------------------------------------
// Pause / admin transfer
// ---------------------------------------------------------------------

use crate::error::StreamError;

fn try_create(ctx: &TestCtx) -> Result<u64, StreamError> {
    match ctx.stream.try_create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &START,
        &END,
        &CLIFF,
        &true,
    ) {
        Ok(Ok(id)) => Ok(id),
        Err(Ok(e)) => Err(e),
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn test_not_paused_by_default() {
    let ctx = setup();
    assert!(!ctx.stream.is_paused());
}

#[test]
fn test_pause_blocks_create_stream_with_typed_error() {
    let ctx = setup();
    ctx.stream.pause();
    assert!(ctx.stream.is_paused());
    assert_eq!(try_create(&ctx), Err(StreamError::Paused));
}

#[test]
fn test_unpause_restores_create_stream() {
    let ctx = setup();
    ctx.stream.pause();
    ctx.stream.unpause();
    assert!(!ctx.stream.is_paused());
    assert_eq!(try_create(&ctx), Ok(1));
}

#[test]
fn test_pause_never_traps_existing_streams() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    ctx.stream.pause();

    // Recipient can still withdraw what has accrued...
    set_ledger(&ctx, START + (END - START) / 2);
    assert_eq!(ctx.stream.withdraw_max(&ctx.recipient, &id), AMOUNT / 2);

    // ...and the sender can still cancel and be refunded.
    ctx.stream.cancel_stream(&ctx.sender, &id);
    assert_eq!(ctx.vault.balance_of_stream(&id), 0);
}

#[test]
fn test_pause_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let stream_id = env.register(StreamContract, ());
    let stream = StreamContractClient::new(&env, &stream_id);
    stream.initialize(&Address::generate(&env), &Address::generate(&env));

    // Drop the blanket auth mock: pause/unpause must now be rejected.
    env.set_auths(&[]);
    assert!(stream.try_pause().is_err());
    assert!(stream.try_unpause().is_err());
}

#[test]
fn test_pause_before_initialize_is_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let stream_id = env.register(StreamContract, ());
    let stream = StreamContractClient::new(&env, &stream_id);
    assert_eq!(stream.try_pause(), Err(Ok(StreamError::NotInitialized)));
    assert_eq!(stream.try_unpause(), Err(Ok(StreamError::NotInitialized)));
}

#[test]
fn test_transfer_admin_changes_admin_and_new_admin_can_pause() {
    let ctx = setup();
    let new_admin = Address::generate(&ctx.env);
    ctx.stream.transfer_admin(&new_admin);
    assert_eq!(ctx.stream.get_admin(), new_admin);
    ctx.stream.pause();
    assert!(ctx.stream.is_paused());
}

#[test]
fn test_pause_unpause_and_transfer_admin_each_emit_one_event() {
    use soroban_sdk::testutils::Events as _;
    let ctx = setup();

    ctx.stream.pause();
    assert_eq!(ctx.env.events().all().events().len(), 1, "pause emits Pause");

    ctx.stream.unpause();
    assert_eq!(
        ctx.env.events().all().events().len(),
        1,
        "unpause emits Unpause"
    );

    let new_admin = Address::generate(&ctx.env);
    ctx.stream.transfer_admin(&new_admin);
    assert_eq!(
        ctx.env.events().all().events().len(),
        1,
        "transfer_admin emits AdminTransfer"
    );
}

#[test]
fn test_transfer_admin_without_auth_fails_and_admin_is_unchanged() {
    let ctx = setup();
    let new_admin = Address::generate(&ctx.env);

    // Drop the blanket auth mock: neither the current admin nor the new admin
    // has authorized the hand-over, so it must be rejected.
    ctx.env.set_auths(&[]);
    assert!(ctx.stream.try_transfer_admin(&new_admin).is_err());
    assert_eq!(ctx.stream.get_admin(), ctx.admin);

    // Authorize both parties and the hand-over succeeds.
    ctx.env.mock_all_auths();
    assert!(ctx.stream.try_transfer_admin(&new_admin).is_ok());
    assert_eq!(ctx.stream.get_admin(), new_admin);
}

// ---------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------

#[test]
fn test_counts_track_each_party_separately() {
    let ctx = setup();
    assert_eq!(ctx.stream.count_streams_by_sender(&ctx.sender), 0);
    create_default_stream(&ctx, true);
    create_default_stream(&ctx, true);
    assert_eq!(ctx.stream.count_streams_by_sender(&ctx.sender), 2);
    assert_eq!(ctx.stream.count_streams_by_recipient(&ctx.recipient), 2);
    assert_eq!(ctx.stream.count_streams_by_sender(&ctx.recipient), 0);
}

#[test]
fn test_paginated_queries_return_creation_order_slices() {
    let ctx = setup();
    for _ in 0..5 {
        create_default_stream(&ctx, true);
    }

    let page = ctx.stream.get_streams_by_sender_page(&ctx.sender, &0, &2);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().id, 1);
    assert_eq!(page.get(1).unwrap().id, 2);

    let page = ctx
        .stream
        .get_streams_by_recipient_page(&ctx.recipient, &3, &10);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().id, 4);
    assert_eq!(page.get(1).unwrap().id, 5);
}

#[test]
fn test_pagination_edge_cases() {
    let ctx = setup();
    create_default_stream(&ctx, true);

    // offset past the end -> empty, not an error
    assert_eq!(
        ctx.stream
            .get_streams_by_sender_page(&ctx.sender, &5, &10)
            .len(),
        0
    );
    // zero limit -> empty
    assert_eq!(
        ctx.stream
            .get_streams_by_sender_page(&ctx.sender, &0, &0)
            .len(),
        0
    );
    // offset + limit overflow must saturate, not panic
    assert_eq!(
        ctx.stream
            .get_streams_by_sender_page(&ctx.sender, &u32::MAX, &u32::MAX)
            .len(),
        0
    );
    // unknown address -> empty
    let stranger = Address::generate(&ctx.env);
    assert_eq!(
        ctx.stream
            .get_streams_by_sender_page(&stranger, &0, &10)
            .len(),
        0
    );
}

#[test]
fn test_page_size_is_capped() {
    let ctx = setup();
    let n = crate::MAX_PAGE_SIZE + 3;
    token::StellarAssetClient::new(&ctx.env, &ctx.token_address)
        .mint(&ctx.sender, &(AMOUNT * n as i128));
    for _ in 0..n {
        create_default_stream(&ctx, true);
    }
    let page = ctx
        .stream
        .get_streams_by_sender_page(&ctx.sender, &0, &u32::MAX);
    assert_eq!(page.len(), crate::MAX_PAGE_SIZE);
}

// ---------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------

#[test]
fn test_create_event_carries_cliff_and_cancellable() {
    use soroban_sdk::{symbol_short, testutils::Events as _, vec};
    let ctx = setup();

    // A cliff distinct from both START and END, and cancellable = false, so a
    // regression that dropped or defaulted either field would be caught.
    let cliff = START + 500;
    let cancellable = false;
    let id = ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &START,
        &END,
        &cliff,
        &cancellable,
    );

    // The indexer reconstructs a stream purely from the create event, so the
    // payload must carry all eight data fields:
    // sender, recipient, token, total_amount, start_ledger, end_ledger,
    // cliff_ledger, cancellable.
    let expected = (
        ctx.stream.address.clone(),
        (symbol_short!("create"), id).into_val(&ctx.env),
        (
            ctx.sender.clone(),
            ctx.recipient.clone(),
            ctx.token_address.clone(),
            AMOUNT,
            START,
            END,
            cliff,
            cancellable,
        )
            .into_val(&ctx.env),
    );

    assert_eq!(ctx.env.events().all(), vec![&ctx.env, expected]);
}

#[test]
fn test_first_stream_id_is_one_and_ids_increase_by_one() {
    let ctx = setup();
    let id1 = create_default_stream(&ctx, true);
    let id2 = create_default_stream(&ctx, false);
    let id3 = create_default_stream(&ctx, true);
    let id4 = create_default_stream(&ctx, true);

    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);
    assert_eq!(id4, 4);
    assert_eq!(ctx.stream.get_stream(&id1).id, 1);
    assert_eq!(ctx.stream.get_stream(&id4).id, 4);
}

#[test]
fn test_create_stream_with_start_in_past_is_immediately_active() {
    let ctx = setup();
    // Creating a stream whose start_ledger is already in the past is
    // deliberately allowed so retroactive/backdated schedules (e.g. vesting
    // or payroll that started before on-chain submission, or transactions
    // that landed a few ledgers after their intended start_ledger) succeed
    // and immediately begin accruing from start_ledger.
    set_ledger(&ctx, 1_500);
    let id = ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &START,
        &END,
        &CLIFF,
        &true,
    );

    assert_eq!(id, 1);
    assert_eq!(ctx.stream.get_status(&id), StreamStatus::Active);
    assert_eq!(ctx.stream.streamed_amount(&id), AMOUNT / 2);
}

#[test]
fn test_withdraw_event_carries_recipient_and_amount() {
    use crate::events;
    use soroban_sdk::{testutils::Events as _, Event as _};

    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);

    let withdraw_amt = AMOUNT / 4;
    ctx.stream.withdraw(&ctx.recipient, &id, &withdraw_amt);

    let emitted = ctx.env.events().all();
    assert_eq!(
        emitted.filter_by_contract(&ctx.stream.address),
        [events::Withdraw {
            stream_id: id,
            recipient: ctx.recipient.clone(),
            amount: withdraw_amt,
        }
        .to_xdr(&ctx.env, &ctx.stream.address)],
    );
}

#[test]
fn test_single_ledger_stream_accrues_and_withdraws_everything_at_end() {
    let ctx = setup();
    let end = START + 1;
    let id = ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &START,
        &end,
        &START,
        &true,
    );

    set_ledger(&ctx, START);
    assert_eq!(ctx.stream.streamed_amount(&id), 0);

    set_ledger(&ctx, end);
    assert_eq!(ctx.stream.streamed_amount(&id), AMOUNT);

    ctx.stream.withdraw(&ctx.recipient, &id, &AMOUNT);
    assert_eq!(ctx.token.balance(&ctx.recipient), AMOUNT);
    assert_eq!(ctx.stream.withdrawable_amount(&id), 0);
    assert_eq!(ctx.vault.balance_of_stream(&id), 0);
}

#[test]
fn test_cancel_event_carries_streamed_now_and_remainder() {
    use crate::events;
    use soroban_sdk::{testutils::Events as _, Event as _};

    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);

    ctx.stream.cancel_stream(&ctx.sender, &id);

    let streamed_now = AMOUNT / 2;
    let remainder = AMOUNT - streamed_now;
    assert_eq!(streamed_now + remainder, AMOUNT);

    let emitted = ctx.env.events().all();
    assert_eq!(
        emitted.filter_by_contract(&ctx.stream.address),
        [events::Cancel {
            stream_id: id,
            sender: ctx.sender.clone(),
            streamed_now,
            remainder,
        }
        .to_xdr(&ctx.env, &ctx.stream.address)],
    );
}

#[test]
fn test_cancel_event_before_cliff_carries_zero_streamed_and_full_remainder() {
    use crate::events;
    use soroban_sdk::{testutils::Events as _, Event as _};

    let ctx = setup();
    let id = ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &1_000,
        &2_000,
        &1_500,
        &true,
    );

    set_ledger(&ctx, 1_200);
    ctx.stream.cancel_stream(&ctx.sender, &id);

    let streamed_now = 0;
    let remainder = AMOUNT;
    assert_eq!(streamed_now + remainder, AMOUNT);

    let emitted = ctx.env.events().all();
    assert_eq!(
        emitted.filter_by_contract(&ctx.stream.address),
        [events::Cancel {
            stream_id: id,
            sender: ctx.sender.clone(),
            streamed_now,
            remainder,
        }
        .to_xdr(&ctx.env, &ctx.stream.address)],
    );
}

/// Regression: cancelling before the cliff used to refund the sender only the
/// unstreamed remainder while `withdrawable_amount` returned 0 forever, leaving
/// the accrued-but-unclaimable part stranded in the vault.
#[test]
fn test_cancel_before_cliff_refunds_the_sender_in_full() {
    let ctx = setup();
    let id = ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &1_000,
        &2_000,
        &1_500, // cliff at the midpoint
        &true,
    );
    set_ledger(&ctx, 1_200); // streaming, but before the cliff
    ctx.stream.cancel_stream(&ctx.sender, &id);

    assert_eq!(
        ctx.token.balance(&ctx.sender),
        AMOUNT * 10,
        "sender made whole"
    );
    assert_eq!(ctx.vault.balance_of_stream(&id), 0, "nothing stranded");
    assert_eq!(ctx.stream.streamed_amount(&id), 0);
    assert_eq!(ctx.stream.withdrawable_amount(&id), 0);

    // and it stays that way once the original cliff and end have passed
    set_ledger(&ctx, 3_000);
    assert_eq!(ctx.stream.withdrawable_amount(&id), 0);
    assert_eq!(ctx.token.balance(&ctx.recipient), 0);
}

// ---------------------------------------------------------------------
// Conservation sweep
// ---------------------------------------------------------------------

/// Tiny deterministic PRNG (PCG-style LCG) so the sweep is reproducible and
/// needs no extra dev-dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

/// `floor(total * elapsed / duration)` -- the most the recipient can ever be
/// entitled to at `seq`, independent of the contract's implementation.
fn reference_streamed(total: i128, start: u32, end: u32, seq: u32) -> i128 {
    if seq <= start {
        return 0;
    }
    let elapsed = i128::from(seq.min(end) - start);
    total * elapsed / i128::from(end - start)
}

/// Whatever mix of schedule, cliff, withdrawals and cancellation is thrown at
/// it, value is neither created nor destroyed, the recipient never holds more
/// than has accrued, accrual never goes backwards, and the vault's per-stream
/// balance always equals what is still owed.
#[test]
fn test_value_is_conserved_across_randomized_schedules() {
    // (total, start, end, cliff, cancel_at)
    let cases: [(i128, u32, u32, u32, Option<u32>); 9] = [
        (1_000_003, 100, 107, 100, None),              // indivisible, tiny
        (1_000_003, 100, 107, 103, None),              // with a cliff
        (9_999_999, 50, 5_000, 2_500, None),           // long, cliff at the midpoint
        (777, 10, 1_000, 10, Some(500)),               // cancelled mid-stream
        (5_000_001, 1, 3, 2, Some(2)),                 // cancelled right at the cliff
        (1_000_000, 1_000, 2_000, 1_500, Some(1_200)), // cancelled before the cliff
        (1_000_000, 1_000, 2_000, 1_000, Some(500)),   // cancelled before it starts
        (10, 1, 4, 1, None),                           // dust amounts
        (2_000_000, 200, 4_000, 200, Some(3_999)),     // cancelled one ledger before the end
    ];

    for (case_no, &(total, start, end, cliff, cancel_at)) in cases.iter().enumerate() {
        let ctx = setup();
        let supply = AMOUNT * 10; // minted to the sender in `setup`
        let id = ctx.stream.create_stream(
            &ctx.sender,
            &ctx.recipient,
            &ctx.token_address,
            &total,
            &start,
            &end,
            &cliff,
            &true,
        );

        let mut rng = Lcg(0x5EED_0000 + case_no as u64);
        let mut cancelled = false;
        let mut frozen: i128 = 0; // streamed amount at cancellation
        let mut last_streamed: i128 = 0;

        let step = ((end - start) / 40).max(1);
        let mut seq = start.saturating_sub(2);
        while seq <= end + 2 {
            set_ledger(&ctx, seq);

            if !cancelled && cancel_at.is_some_and(|c| seq >= c) {
                ctx.stream.cancel_stream(&ctx.sender, &id);
                cancelled = true;
                frozen = ctx.stream.streamed_amount(&id);
                // A pre-cliff cancel voids the recipient's accrual (it drops to 0), so the
                // "never goes backwards" baseline restarts at the frozen value.
                last_streamed = frozen;
                // Cancelling can never hand the recipient more than accrued.
                assert!(
                    frozen <= reference_streamed(total, start, end, seq),
                    "case {case_no}"
                );
                // The sender was refunded exactly the unstreamed remainder.
                assert_eq!(
                    ctx.token.balance(&ctx.sender),
                    supply - frozen,
                    "case {case_no}"
                );
            }

            match rng.next() % 3 {
                0 => {
                    let w = ctx.stream.withdrawable_amount(&id);
                    if w > 0 {
                        let amount = 1 + i128::from(rng.next() % w as u64);
                        ctx.stream.withdraw(&ctx.recipient, &id, &amount);
                    }
                }
                1 => {
                    ctx.stream.withdraw_max(&ctx.recipient, &id);
                }
                _ => {}
            }

            let received = ctx.token.balance(&ctx.recipient);
            let in_vault = ctx.token.balance(&ctx.vault.address);
            let streamed = ctx.stream.streamed_amount(&id);

            // 1. value is conserved
            assert_eq!(
                ctx.token.balance(&ctx.sender) + received + in_vault,
                supply,
                "case {case_no} seq {seq}: tokens created or destroyed"
            );
            // 2. recipient never holds more than has accrued
            assert!(received <= streamed, "case {case_no} seq {seq}: overpaid");
            assert!(
                received <= total,
                "case {case_no} seq {seq}: paid beyond deposit"
            );
            // 3. accrual only moves forward
            assert!(
                streamed >= last_streamed,
                "case {case_no} seq {seq}: accrual went backwards"
            );
            last_streamed = streamed;
            // 4. the vault holds exactly what is still owed on this stream
            let owed = if cancelled {
                frozen - received
            } else {
                total - received
            };
            assert_eq!(
                ctx.vault.balance_of_stream(&id),
                owed,
                "case {case_no} seq {seq}"
            );

            seq += step;
        }

        // Past the end (or after cancelling) the recipient can drain what is owed,
        // and nothing is left stranded in the vault.
        set_ledger(&ctx, end + 10);
        ctx.stream.withdraw_max(&ctx.recipient, &id);
        let expected_to_recipient = if cancelled { frozen } else { total };
        assert_eq!(
            ctx.token.balance(&ctx.recipient),
            expected_to_recipient,
            "case {case_no}"
        );
        assert_eq!(ctx.vault.balance_of_stream(&id), 0, "case {case_no}");
        assert_eq!(
            ctx.token.balance(&ctx.sender),
            supply - expected_to_recipient,
            "case {case_no}"
        );
    }
}

// ---------------------------------------------------------------------
// Cancel edge cases (#29, #30, #31, #32)
// ---------------------------------------------------------------------

/// #29 — cancel_stream on an unknown stream id returns StreamNotFound.
#[test]
fn test_cancel_stream_unknown_id_returns_stream_not_found() {
    let ctx = setup();
    set_ledger(&ctx, START + 100);
    assert_eq!(
        ctx.stream.try_cancel_stream(&ctx.sender, &999),
        Err(Ok(StreamError::StreamNotFound)),
    );
}

/// #30 — cancelling an already-cancelled stream returns AlreadyFinalized.
#[test]
fn test_cancel_stream_twice_returns_already_finalized() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);

    // First cancel — must succeed.
    ctx.stream.cancel_stream(&ctx.sender, &id);

    // Second cancel — must return AlreadyFinalized; no extra refund.
    let sender_balance_after_first = ctx.token.balance(&ctx.sender);
    assert_eq!(
        ctx.stream.try_cancel_stream(&ctx.sender, &id),
        Err(Ok(StreamError::AlreadyFinalized)),
    );
    // Sender balance unchanged by the failed second cancel.
    assert_eq!(ctx.token.balance(&ctx.sender), sender_balance_after_first);
}

/// #31 — after a cancel the recipient can still withdraw the frozen share.
#[test]
fn test_recipient_can_withdraw_frozen_share_after_cancel() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);

    // Cancel at midpoint — half is frozen for the recipient.
    ctx.stream.cancel_stream(&ctx.sender, &id);
    let frozen = ctx.stream.streamed_amount(&id); // AMOUNT / 2
    assert_eq!(frozen, AMOUNT / 2);

    // Advance ledger well past END; frozen share must not change.
    set_ledger(&ctx, END + 1_000);
    assert_eq!(ctx.stream.withdrawable_amount(&id), frozen);

    // Recipient withdraws the full frozen share.
    let withdrawn = ctx.stream.withdraw_max(&ctx.recipient, &id);
    assert_eq!(withdrawn, frozen);
    assert_eq!(ctx.token.balance(&ctx.recipient), frozen);

    // Vault balance for this stream must now be zero.
    assert_eq!(ctx.vault.balance_of_stream(&id), 0);
}

/// #32 — after a cancel, ledger progress cannot raise what the recipient may
/// withdraw; attempting to take more than the frozen share returns
/// ExceedsWithdrawable.
#[test]
fn test_recipient_cannot_withdraw_more_than_frozen_share_after_cancel() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    let midpoint = START + (END - START) / 2;
    set_ledger(&ctx, midpoint);

    ctx.stream.cancel_stream(&ctx.sender, &id);
    let frozen = ctx.stream.streamed_amount(&id); // AMOUNT / 2

    // Move far past END — withdrawable must stay at `frozen`.
    set_ledger(&ctx, END + 100_000);
    assert_eq!(ctx.stream.withdrawable_amount(&id), frozen);

    // Trying to withdraw frozen + 1 must fail with ExceedsWithdrawable.
    assert_eq!(
        ctx.stream.try_withdraw(&ctx.recipient, &id, &(frozen + 1)),
        Err(Ok(StreamError::ExceedsWithdrawable)),
    );
}

// ---------------------------------------------------------------------
// Boundary tests (#33, #34, #35, #36)
// ---------------------------------------------------------------------

/// #33 — Cancelling exactly at start_ledger (with cliff == start) refunds
/// the full deposit to the sender: elapsed is 0, so streamed_now is 0 and
/// the entire amount is the remainder.
#[test]
fn test_cancel_at_start_ledger_refunds_everything() {
    let ctx = setup();
    let balance_before = ctx.token.balance(&ctx.sender);

    // CLIFF == START (the default create_default_stream parameters).
    let id = create_default_stream(&ctx, true);
    set_ledger(&ctx, START);

    ctx.stream.cancel_stream(&ctx.sender, &id);

    // The cancel event must report streamed_now == 0.
    let stream = ctx.stream.get_stream(&id);
    assert_eq!(ctx.stream.streamed_amount(&id), 0);

    // Sender must be exactly as rich as before the stream was created.
    assert_eq!(ctx.token.balance(&ctx.sender), balance_before);

    // Nothing left in the vault for this stream.
    assert_eq!(ctx.vault.balance_of_stream(&id), 0);

    // Recipient can withdraw nothing.
    assert_eq!(ctx.stream.withdrawable_amount(&id), 0);
    assert_eq!(stream.status, StreamStatus::Cancelled);
}

/// #34 — Cancelling exactly at end_ledger returns AlreadyFinalized because
/// the stream's derived status is Completed at now >= end_ledger.
#[test]
fn test_cancel_at_end_ledger_returns_already_finalized() {
    let ctx = setup();
    let id = create_default_stream(&ctx, true);
    set_ledger(&ctx, END);

    assert_eq!(
        ctx.stream.try_cancel_stream(&ctx.sender, &id),
        Err(Ok(StreamError::AlreadyFinalized)),
    );

    // The stream must still be Completed (not mutated by the failed cancel).
    assert_eq!(ctx.stream.get_status(&id), StreamStatus::Completed);
}

/// #35 — create_stream must accept cliff == start_ledger and cliff == end_ledger.
/// Both are valid per the condition `cliff < start || cliff > end` (exclusive).
#[test]
fn test_create_stream_accepts_cliff_equal_to_start() {
    let ctx = setup();
    let id = ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &START,
        &END,
        &START, // cliff == start
        &true,
    );
    let stream = ctx.stream.get_stream(&id);
    assert_eq!(stream.cliff_ledger, START);
}

#[test]
fn test_create_stream_accepts_cliff_equal_to_end() {
    let ctx = setup();
    let id = ctx.stream.create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &AMOUNT,
        &START,
        &END,
        &END, // cliff == end
        &true,
    );
    let stream = ctx.stream.get_stream(&id);
    assert_eq!(stream.cliff_ledger, END);
}

/// #36 — create_stream must reject a negative total_amount (the guard is
/// `total_amount <= 0`, so –1 must return InvalidAmount just like 0).
#[test]
fn test_create_stream_rejects_negative_amount() {
    let ctx = setup();
    let balance_before = ctx.token.balance(&ctx.sender);

    let result = ctx.stream.try_create_stream(
        &ctx.sender,
        &ctx.recipient,
        &ctx.token_address,
        &(-1_i128),
        &START,
        &END,
        &CLIFF,
        &true,
    );

    assert_eq!(result, Err(Ok(StreamError::InvalidAmount)));

    // No tokens must have moved.
    assert_eq!(ctx.token.balance(&ctx.sender), balance_before);
}
