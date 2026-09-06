# Stream Contract

`contracts/stream` — crate name `stellar-stream-hive-stream`.

## Overview

The stream contract is the heart of Stellar Stream Hive. It lets a
`sender` commit `total_amount` of a token to a `recipient`, streamed
linearly, ledger by ledger, between `start_ledger` and `end_ledger`. At any
point after an optional `cliff_ledger`, the recipient can withdraw
whatever has accrued and not yet been withdrawn.

The stream contract never holds funds itself. On `create_stream` it
instructs the [vault contract](./vault.md) to pull the deposit; on
`withdraw`/`cancel_stream` it instructs the vault to pay out. This
contract is purely the bookkeeping and math layer.

## The streaming formula

```text
elapsed        = min(current_ledger, end_ledger) - start_ledger   (clamped to 0 before start)
total_duration = end_ledger - start_ledger
streamed       = total_amount * elapsed / total_duration
withdrawable   = max(streamed - withdrawn, 0)                     (0 before cliff_ledger)
```

Implemented in `contracts/stream/src/math.rs` as pure, `Env`-independent
functions so the arithmetic can be exhaustively unit tested in isolation
from contract/storage concerns.

Every step uses `i128` checked arithmetic (`checked_mul`, `checked_div`,
`checked_add`, `checked_sub`) — never raw operators, never `unwrap()` on a
computation that could realistically overflow. The multiplication
(`total_amount * elapsed`) always happens **before** the division (`/
total_duration`), which avoids the precision loss that `(total_amount /
total_duration) * elapsed` would introduce for any amount that doesn't
divide evenly.

### Worked example

A payroll stream of `90,000` USDC (6 decimals, but we'll use whole units
for clarity) running for a 30-day pay period, ledgers `1,000` through
`519,000` (30 days at 5s ledgers ≈ 518,400 ledgers — rounded here for
readability), no cliff:

- At ledger `1,000` (the start): `elapsed = 0` → `streamed = 0`.
- Halfway through, ledger `260,000`: `elapsed = 259,000`,
  `total_duration = 518,000`, `streamed = 90,000 * 259,000 / 518,000 =
  44,990` (not exactly half due to integer rounding — this is expected and
  the whole reason we use `checked_div`, which always rounds down, never
  up: the recipient is never able to withdraw more than has truly accrued).
- At or after ledger `519,000`: `streamed = 90,000` (the clamp via
  `min(current_ledger, end_ledger)` caps `elapsed` at `total_duration`).

If the recipient withdraws `20,000` at the halfway point, `withdrawn`
becomes `20,000`, and `withdrawable_amount` at that same instant becomes
`44,990 - 20,000 = 24,990`.

### Cancellation freezing

If the sender cancels at ledger `260,000`, the contract:

1. Computes `streamed_now = 44,990` (as above) using the real current
   ledger — this is the last moment `streamed_amount` is computed against
   live time.
2. Refunds `total_amount - streamed_now = 45,010` back to the sender
   immediately via the vault.
3. Marks the stream `Cancelled` and records `cancelled_at_ledger =
   260,000`.

From that point on, `streamed_amount` for this stream ignores the real
ledger entirely and always recomputes the formula using
`cancelled_at_ledger` in place of "now." This freezes the streamed value
at `44,990` forever — the recipient can still withdraw up to that amount
(minus whatever they'd already withdrawn), but ledger progress after
cancellation can never inflate it further.

## Data model

```rust
pub enum StreamStatus { Pending, Active, Completed, Cancelled }

pub struct Stream {
    pub id: u64,
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    pub total_amount: i128,
    pub withdrawn: i128,
    pub start_ledger: u32,
    pub end_ledger: u32,
    pub cliff_ledger: u32,
    pub cancellable: bool,
    pub status: StreamStatus,
    pub created_at: u32,
    pub cancelled_at_ledger: u32, // 0 unless cancelled
}
```

`Pending`, `Active`, and `Completed` are **derived**, not stored — they are
purely a function of the current ledger versus `start_ledger`/
`end_ledger` (see `math::compute_status`). `Cancelled` is the one status
that genuinely needs to be persisted, since it can't be reconstructed from
time alone. `get_stream`, `get_status`, and the `get_streams_by_*`
functions all recompute the live status on every read.

## Function reference

### `initialize(env, admin, vault)`
One-time setup, requires `admin.require_auth()`.

### `create_stream(env, sender, recipient, token, total_amount, start_ledger, end_ledger, cliff_ledger, cancellable) -> u64`
Requires `sender.require_auth()`. Validates `total_amount > 0`,
`end_ledger > start_ledger`, and `start_ledger <= cliff_ledger <=
end_ledger`. Deposits `total_amount` into the vault, stores the new
`Stream`, indexes it under both sender and recipient, and returns its id.
Emits a `create` event.

### `streamed_amount(env, stream_id) -> i128`
Read-only. See [formula](#the-streaming-formula) above.

### `withdrawable_amount(env, stream_id) -> i128`
Read-only. `streamed_amount - withdrawn`, floored at 0, and forced to 0
before `cliff_ledger`.

### `withdraw(env, recipient, stream_id, amount)`
Requires `recipient.require_auth()` and that the caller is the stream's
recipient. Rejects `amount` greater than `withdrawable_amount`
(`ExceedsWithdrawable`). Instructs the vault to `release` the funds.
Emits a `withdraw` event.

### `withdraw_max(env, recipient, stream_id) -> i128`
Convenience wrapper around `withdraw` that pulls the entire currently
withdrawable balance and returns the amount withdrawn (0 if nothing was
available — this is not an error).

### `cancel_stream(env, sender, stream_id)`
Requires `sender.require_auth()` and that the caller is the stream's
sender. Fails with `NotCancellable` if the stream was created with
`cancellable = false`, and `AlreadyFinalized` if the stream is already
`Cancelled` or `Completed`. Freezes the streamed amount (see
[Cancellation freezing](#cancellation-freezing)), refunds the unstreamed
remainder to the sender via the vault, and emits a `cancel` event.

### `get_stream(env, stream_id) -> Stream`
Read-only, with `status` recomputed live.

### `get_streams_by_sender(env, sender) -> Vec<Stream>` / `get_streams_by_recipient(env, recipient) -> Vec<Stream>`
Read-only, indexed lookups.

### `get_status(env, stream_id) -> StreamStatus`
Read-only, live-computed status.

## Events

| Topic | Payload | Emitted when |
|---|---|---|
| `("create", stream_id)` | `(sender, recipient, token, total_amount, start_ledger, end_ledger)` | `create_stream` succeeds |
| `("withdraw", stream_id)` | `(recipient, amount)` | `withdraw` (or `withdraw_max`) succeeds |
| `("cancel", stream_id)` | `(sender, streamed_now, remainder)` | `cancel_stream` succeeds |

## Cross-contract calls to the vault

The stream contract calls the vault purely via `Env::invoke_contract` by
address and function-name symbol (see `contracts/stream/src/vault_client.rs`),
rather than depending on the vault crate as a Rust library dependency.
This keeps the two contracts fully decoupled at compile time — no fixed
build order, no WASM-artifact dependency between crates — while producing
exactly the same on-chain call as a generated `Client` would. Integration
tests use the real vault contract (via a `dev-dependency` on the vault
crate, only active for `cargo test`, never for the WASM release build) so
the full deposit → withdraw → release path is tested end-to-end.

## Security notes

- Every state-changing function that acts on behalf of a specific party
  calls `require_auth()` on that party's address before doing anything
  else: `sender` for `create_stream`/`cancel_stream`, `recipient` for
  `withdraw`/`withdraw_max`, `admin` for `initialize`.
- The contract additionally checks that the authenticated address matches
  the stream's recorded `sender`/`recipient` — `require_auth()` alone only
  proves identity, not authorization for a specific stream.
- All balance arithmetic uses checked i128 operations; overflow/underflow
  returns `StreamError::MathOverflow` instead of panicking or wrapping.
- The contract can never cause the vault to pay out more than a stream's
  deposited balance, because `withdrawable_amount` is bounded by
  `streamed_amount <= total_amount`, and `total_amount` is exactly what
  was deposited on creation.
