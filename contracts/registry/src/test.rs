#![cfg(test)]

use crate::{RegistryContract, RegistryContractClient, RegistryError, StreamStatus};
use soroban_sdk::{testutils::Address as _, Address, Env};

struct TestCtx {
    env: Env,
    registry: RegistryContractClient<'static>,
    admin: Address,
}

fn setup() -> TestCtx {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(RegistryContract, ());
    let registry = RegistryContractClient::new(&env, &id);
    registry.initialize(&admin);
    TestCtx {
        env,
        registry,
        admin,
    }
}

#[test]
fn test_initialize_sets_empty_stats() {
    let ctx = setup();
    let stats = ctx.registry.get_stats();
    assert_eq!(stats.total_streams, 0);
    assert_eq!(stats.active_streams, 0);
    assert_eq!(stats.total_streamed, 0);
    assert_eq!(stats.total_withdrawn, 0);
}

#[test]
fn test_initialize_twice_rejected() {
    let ctx = setup();
    let result = ctx.registry.try_initialize(&ctx.admin);
    assert_eq!(result, Err(Ok(RegistryError::AlreadyInitialized)));
}

#[test]
fn test_get_active_streams_empty_after_initialize() {
    let ctx = setup();
    let active = ctx.registry.get_active_streams();
    assert_eq!(active, soroban_sdk::Vec::<u64>::new(&ctx.env));
    assert_eq!(active.len(), 0);
    assert_eq!(ctx.registry.stream_count(), 0);
}

#[test]
fn test_register_stream_adds_to_all_streams() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);

    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);

    let all = ctx.registry.get_all_streams();
    assert_eq!(all.len(), 1);
    assert_eq!(all.get(0).unwrap(), 1);
}

#[test]
fn test_register_stream_updates_stats() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);

    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);
    ctx.registry
        .register_stream(&2, &sender, &recipient, &token, &500);

    let stats = ctx.registry.get_stats();
    assert_eq!(stats.total_streams, 2);
    assert_eq!(stats.active_streams, 2);
    assert_eq!(stats.total_streamed, 1500);
}

#[test]
fn test_register_stream_rejects_negative_amount() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);

    let before = ctx.registry.get_stats();

    let result = ctx
        .registry
        .try_register_stream(&1, &sender, &recipient, &token, &-1);
    assert_eq!(result, Err(Ok(RegistryError::InvalidAmount)));

    let after = ctx.registry.get_stats();
    assert_eq!(after.total_streams, before.total_streams);
    assert_eq!(after.active_streams, before.active_streams);
    assert_eq!(after.total_streamed, before.total_streamed);
    assert_eq!(after.total_withdrawn, before.total_withdrawn);
}

#[test]
#[should_panic]
fn test_register_stream_rejects_duplicate_id() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);

    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);
    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &500);
}

#[test]
fn test_update_stream_tracks_withdrawn_delta() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);
    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);

    ctx.registry.update_stream(&1, &300, &StreamStatus::Active);
    assert_eq!(ctx.registry.get_stats().total_withdrawn, 300);

    ctx.registry.update_stream(&1, &700, &StreamStatus::Active);
    assert_eq!(ctx.registry.get_stats().total_withdrawn, 700);
}

#[test]
fn test_lowering_withdrawn_does_not_reduce_total_withdrawn() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);
    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);

    ctx.registry.update_stream(&1, &100, &StreamStatus::Active);
    ctx.registry.update_stream(&1, &40, &StreamStatus::Active);

    assert_eq!(ctx.registry.get_stream_record(&1).withdrawn, 40);
    assert_eq!(ctx.registry.get_stats().total_withdrawn, 100);
}

#[test]
fn test_update_stream_rejects_negative_withdrawn() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);
    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);

    let before = ctx.registry.get_stats();

    let result = ctx
        .registry
        .try_update_stream(&1, &-1, &StreamStatus::Active);
    assert_eq!(result, Err(Ok(RegistryError::InvalidAmount)));

    let after = ctx.registry.get_stats();
    assert_eq!(after.total_streams, before.total_streams);
    assert_eq!(after.active_streams, before.active_streams);
    assert_eq!(after.total_streamed, before.total_streamed);
    assert_eq!(after.total_withdrawn, before.total_withdrawn);
    assert_eq!(ctx.registry.get_stream_record(&1).withdrawn, 0);
}

#[test]
fn test_get_admin_before_initialize_returns_not_initialized() {
    let env = Env::default();
    let id = env.register(RegistryContract, ());
    let registry = RegistryContractClient::new(&env, &id);

    assert_eq!(registry.try_get_admin(), Err(Ok(RegistryError::NotInitialized)));
}

#[test]
fn test_update_stream_to_pending_decrements_active_count() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);
    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);
    assert_eq!(ctx.registry.get_stats().active_streams, 1);

    ctx.registry.update_stream(&1, &0, &StreamStatus::Pending);
    assert_eq!(ctx.registry.get_stats().active_streams, 0);

    ctx.registry.update_stream(&1, &0, &StreamStatus::Active);
    assert_eq!(ctx.registry.get_stats().active_streams, 1);
}

#[test]
fn test_cancelled_to_completed_keeps_active_count_at_zero() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);
    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);

    ctx.registry
        .update_stream(&1, &0, &StreamStatus::Cancelled);
    assert_eq!(ctx.registry.get_stats().active_streams, 0);

    ctx.registry
        .update_stream(&1, &0, &StreamStatus::Completed);
    assert_eq!(ctx.registry.get_stats().active_streams, 0);
}

#[test]
fn test_update_stream_to_completed_decrements_active_count() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);
    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);
    assert_eq!(ctx.registry.get_stats().active_streams, 1);

    ctx.registry
        .update_stream(&1, &1000, &StreamStatus::Completed);
    assert_eq!(ctx.registry.get_stats().active_streams, 0);
}

#[test]
fn test_update_stream_to_cancelled_decrements_active_count() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);
    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);

    ctx.registry
        .update_stream(&1, &400, &StreamStatus::Cancelled);
    let stats = ctx.registry.get_stats();
    assert_eq!(stats.active_streams, 0);
    assert_eq!(stats.total_withdrawn, 400);
}

#[test]
fn test_update_stream_rejects_unknown_id() {
    let ctx = setup();
    let result = ctx
        .registry
        .try_update_stream(&999, &100, &StreamStatus::Active);
    assert_eq!(result, Err(Ok(RegistryError::StreamNotFound)));
}

#[test]
fn test_get_stream_record_rejects_unknown_id() {
    let ctx = setup();
    let result = ctx.registry.try_get_stream_record(&999);
    assert_eq!(result, Err(Ok(RegistryError::StreamNotFound)));
}

#[test]
fn test_get_active_streams_filters_correctly() {
    let ctx = setup();
    let sender = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let token = Address::generate(&ctx.env);
    ctx.registry
        .register_stream(&1, &sender, &recipient, &token, &1000);
    ctx.registry
        .register_stream(&2, &sender, &recipient, &token, &2000);
    ctx.registry
        .register_stream(&3, &sender, &recipient, &token, &3000);

    ctx.registry
        .update_stream(&2, &2000, &StreamStatus::Completed);

    let active = ctx.registry.get_active_streams();
    assert_eq!(active.len(), 2);
    assert_eq!(active.get(0).unwrap(), 1);
    assert_eq!(active.get(1).unwrap(), 3);
}
