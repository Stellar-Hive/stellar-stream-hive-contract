# Vault Contract

`contracts/vault` — crate name `stellar-stream-hive-vault`.

## Overview

The vault is the single custodian of every token ever locked into a
Stellar Stream Hive payment stream. It does not know anything about
streaming math, cliffs, or schedules — that logic lives entirely in the
[stream contract](./stream.md). The vault's job is narrow and boring on
purpose:

1. Accept deposits of a token, attributed to a `stream_id`.
2. Release part of a stream's locked balance to a recipient on withdrawal.
3. Refund part of a stream's locked balance to the original sender on
   cancellation.
4. Report balances (per-stream and per-token totals) for anyone to query.

Keeping the vault this simple is deliberate: the smaller and more
mechanical the code that actually moves money, the easier it is to audit
and the smaller the attack surface.

## The security boundary

**This is the most important part of the whole system.** [`release`](#release)
and [`refund`](#refund) are the only two functions that move funds *out* of
the vault. Both:

1. Require the `caller` argument to cryptographically authenticate via
   `caller.require_auth()`.
2. Reject the call with `VaultError::Unauthorized` unless `caller` equals
   the single `stream_contract` address recorded at [`initialize`](#initialize)
   (or later changed via [`set_stream_contract`](#set_stream_contract)).

```rust
caller.require_auth();
if caller != stream_contract {
    return Err(VaultError::Unauthorized);
}
```

Both checks matter independently:

- Without the `require_auth()` call, anyone could *claim* to be the stream
  contract's address as a plain argument, since arguments are just data.
- Without the identity comparison, any address that can produce a valid
  signature for itself (i.e. everyone) could call `release`/`refund`, since
  `require_auth()` on its own only proves "the caller is who they say they
  are" — not "the caller is allowed to do this."

Together, they mean the only entity on all of Stellar that can ever move
money out of the vault is the exact contract address configured as
`stream_contract`. Everyone else — including the vault's own admin — is
rejected before any token transfer happens.

`deposit` is intentionally *not* gated the same way: it only requires
`from.require_auth()`, since a deposit only ever moves the depositor's own
funds into the vault under a `stream_id` of their choosing. In the worst
case, someone deposits their own tokens against an arbitrary stream id;
that cannot be used to drain anyone else's funds, because withdrawals are
still bounded by the stream contract's own accrual math on the other side
of the `release` call, not by whatever balance happens to sit in the
vault.

## Function reference

### `initialize(env, admin: Address, stream_contract: Address)`
One-time setup. Requires `admin.require_auth()`. Fails with
`AlreadyInitialized` if called twice.

### `deposit(env, from: Address, token: Address, amount: i128, stream_id: u64)`
Pulls `amount` of `token` from `from` into the vault via the token's SEP-41
`transfer`, and credits `stream_id`'s locked balance. Requires
`from.require_auth()`. The first deposit for a `stream_id` fixes the token
it is denominated in; subsequent deposits with a different token are
rejected with `TokenMismatch`.

### `release(env, caller: Address, to: Address, token: Address, amount: i128, stream_id: u64)`
Moves `amount` of `token` out of `stream_id`'s locked balance to `to`. See
[Security boundary](#the-security-boundary) above. Fails with
`InsufficientBalance` if `amount` exceeds the stream's current locked
balance.

### `refund(env, caller: Address, to: Address, token: Address, amount: i128, stream_id: u64)`
Identical mechanics and caller check to `release`; used when a cancelled
stream's unstreamed remainder is returned to the sender.

### `balance_of_stream(env, stream_id: u64) -> i128`
Read-only. Current locked balance attributed to a stream (0 if unknown).

### `total_locked(env, token: Address) -> i128`
Read-only. Sum of every stream's locked balance in that token.

### `set_stream_contract(env, admin: Address, new_contract: Address)`
Admin-only. Repoints the address authorized to call `release`/`refund`,
e.g. when upgrading the stream contract without redeploying (and thus
without migrating) the vault. Requires `admin.require_auth()` and that
`admin` matches the stored admin.

### `get_stream_contract(env) -> Address` / `get_admin(env) -> Address`
Read-only accessors.

## Events

Each event is a typed `#[contractevent]` struct (see `contracts/vault/src/events.rs`),
so it appears in the contract's interface spec rather than as an untyped
tuple. Topics are the struct name in snake_case followed by any `#[topic]`
fields; data is the remaining fields.

| Event struct | Topic | Data | Emitted when |
|---|---|---|---|
| `Deposit` | `("deposit", stream_id)` | `(from, token, amount)` | `deposit` succeeds |
| `Release` | `("release", stream_id)` | `(to, token, amount)` | `release` succeeds |
| `Refund` | `("refund", stream_id)` | `(to, token, amount)` | `refund` succeeds |
| `SetStreamContract` | `("set_stream_contract",)` | `new_contract` | `set_stream_contract` succeeds |

## Storage layout

All keys are namespaced under a `DataKey` enum:

- `Admin` — instance storage, the admin address.
- `StreamContract` — instance storage, the sole authorized caller of
  `release`/`refund`.
- `StreamBalance(stream_id)` — persistent storage, locked balance per
  stream.
- `StreamToken(stream_id)` — persistent storage, the token a stream is
  denominated in (set on first deposit, checked on every subsequent call).
- `TotalLocked(token)` — persistent storage, running total per token.

## Error reference

See `contracts/vault/src/error.rs` for the full `VaultError` enum:
`AlreadyInitialized`, `NotInitialized`, `Unauthorized`, `InvalidAmount`,
`InsufficientBalance`, `MathOverflow`, `TokenMismatch`.
