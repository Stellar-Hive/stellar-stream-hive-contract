# Contributing to Stellar Stream Hive Contracts

Thanks for taking a look. This repo holds the three Soroban smart
contracts (`vault`, `stream`, `registry`) that power Stellar Stream Hive's
real-time payment streaming. Sister repos:

- API/indexer: https://github.com/Stellar-Hive/stellar-stream-hive-api
- Frontend: https://github.com/Stellar-Hive/stellar-stream-hive-frontend

## Local dev setup

1. **Install Rust via rustup**: https://rustup.rs
2. **Add the WASM target**, which is what contracts actually compile to
   for deployment:
   ```bash
   rustup target add wasm32v1-none
   ```
   Note: this is `wasm32v1-none`, not the older `wasm32-unknown-unknown`.
   `soroban-sdk` 27.x refuses to build for `wasm32-unknown-unknown` on
   Rust 1.82+ because that target now enables WASM reference-types and
   multi-value by default, which the Soroban host environment doesn't yet
   support; `wasm32v1-none` is the "no new features" WASM target Soroban
   expects. This repo's `rust-toolchain.toml` already pins the right one.
3. **Install the Stellar CLI**, used for local sandboxes, optimizing WASM,
   and deploying:
   ```bash
   cargo install stellar-cli --locked
   stellar --version
   ```
4. **Clone and build**:
   ```bash
   git clone https://github.com/Stellar-Hive/stellar-stream-hive-contract.git
   cd stellar-stream-hive-contract
   cargo build
   ```

## Pinned Rust toolchain

The Rust version is pinned in `rust-toolchain.toml` (and mirrored in
`.github/workflows/ci.yml`) so that clippy lints and WASM output stay
reproducible across releases. To bump it, pick a newer stable version that
builds and passes CI locally (`cargo test --workspace`, `cargo fmt --all --
--check`, and the clippy command from the `justfile`), then update the
`channel` in `rust-toolchain.toml` and the matching toolchain version in
`ci.yml` in the same PR.

## Build, test, deploy

The `justfile` wraps the commands below: `just test`, `just fmt`, `just lint`, `just wasm`, and
`just check` to run everything CI runs. Install [just](https://just.systems) to use it.

```bash
# Run every test in the workspace (vault, stream, registry)
cargo test

# Run just one contract's tests
cargo test -p stellar-stream-hive-stream

# Run one test by name. The argument is a substring match, so this
# also runs any test whose name contains it.
cargo test -p stellar-stream-hive-stream test_withdraw_rejects_non_recipient_caller

# Show println! and debug output from a test (it is captured by default)
cargo test -p stellar-stream-hive-stream test_withdraw_rejects_non_recipient_caller -- --nocapture

# Compile every contract to the real deployment artifact
cargo build --target wasm32v1-none --release \
  -p stellar-stream-hive-vault \
  -p stellar-stream-hive-stream \
  -p stellar-stream-hive-registry

# Deploy to testnet (requires the stellar CLI and a funded identity —
# see scripts/deploy.sh's header comment for details)
stellar keys generate deployer --network testnet --fund
scripts/deploy.sh testnet deployer
```

Running the tests writes a `test_snapshots/` directory into each crate. The
Soroban test environment generates these JSON files to record ledger and
event state for each test. They are generated output, so do not commit them.
They are listed in `.gitignore`, and you can delete them at any time. The next
test run recreates them.

A contract that only passes `cargo test` but fails to compile to
`wasm32v1-none` is not done — that WASM build is the actual thing
that gets deployed, and `#![no_std]` + Soroban SDK code can behave (or
fail to compile) subtly differently under the WASM target than under your
native host target. Always check both before calling something finished.

## Before opening a pull request

Passing tests are not enough: CI (`.github/workflows/ci.yml`) also fails a
PR on `cargo fmt --all -- --check` and on clippy with `-D warnings`. Run
the exact commands CI runs before pushing — these are the same two
commands the `just lint` recipe wraps (the pinned `rust-toolchain.toml`
installs `rustfmt` and `clippy` for you; if you override the toolchain,
add them with `rustup component add rustfmt clippy`):

```bash
# Check formatting across the whole workspace.
cargo fmt --all -- --check

# Lint with every warning denied. The two -A flags match CI exactly:
# they exempt too_many_arguments and needless_borrows_for_generic_args,
# which Soroban's ABI-driven function signatures and client call style
# trip by construction.
cargo clippy --workspace --all-targets -- -D warnings \
  -A clippy::too_many_arguments \
  -A clippy::needless_borrows_for_generic_args
```

If the formatting check fails, `cargo fmt --all` (the same command without
`--check`) rewrites the files in place and fixes every violation it just
reported. If clippy flags code you did not mean to touch, fix the warning
or refactor — the two `-A` exemptions above are the only ones CI makes.

`just check` runs all three gates in one shot: lint (fmt + clippy),
`cargo test --workspace`, and the `wasm32v1-none` build. If you are not
using `just`, run those three by hand as spelled out above and in
"Build, test, deploy".

## Dependency audits

`cargo audit` runs on every pull request and weekly on a schedule
(`.github/workflows/audit.yml`), checking the dependency tree against the
RustSec advisory database.

If it flags an advisory that doesn't apply here, acknowledge it explicitly
rather than letting the job stay red: add the advisory id (e.g.
`RUSTSEC-0000-0000`) to the `ignore` list in `audit.toml`, with a one-line
comment explaining why it doesn't apply and, if there is one, a link to an
upstream issue tracking a real fix. Never disable or remove the audit job to
make a failure go away.

## The streaming math, explained

Every stream accrues linearly between `start_ledger` and `end_ledger`:

```text
elapsed        = min(current_ledger, end_ledger) - start_ledger
total_duration = end_ledger - start_ledger
streamed       = total_amount * elapsed / total_duration
withdrawable   = streamed - withdrawn        (0 before cliff_ledger)
```

**Worked example**: a stream of `90,000` units running from ledger
`1,000` to `519,000` (no cliff). At ledger `260,000`:

```
elapsed        = min(260_000, 519_000) - 1_000 = 259_000
total_duration = 519_000 - 1_000               = 518_000
streamed       = 90_000 * 259_000 / 518_000    = 44_990   (rounds down)
```

If the recipient has already withdrawn `20,000`, then
`withdrawable = 44_990 - 20_000 = 24_990`.

Two rules make this safe to implement in fixed-point integer math:

1. **Multiply before you divide.** `total_amount * elapsed / total_duration`
   preserves precision; `(total_amount / total_duration) * elapsed` would
   floor the per-ledger rate first and systematically under- or overpay
   depending on rounding, and can hand out `0` per-ledger accrual entirely
   for amounts smaller than the number of ledgers in the stream.
2. **Use checked arithmetic everywhere.** `total_amount * elapsed` can be
   large; on `i128` it won't realistically overflow for any sane token
   supply, but the contract still uses `checked_mul`/`checked_div`/
   `checked_add`/`checked_sub` throughout and returns a typed error
   instead of panicking or silently wrapping if it ever does.

See `contracts/stream/src/math.rs` for the implementation and its unit
tests, and `docs/stream.md` for the full write-up including how
cancellation freezes the streamed amount.

## The vault's security boundary

The vault is the only contract that actually moves tokens. Its `release`
and `refund` functions — the only ways funds leave it — require the
caller to both (a) cryptographically authenticate (`caller.require_auth()`)
and (b) match the single `stream_contract` address configured at
`initialize`. Anyone else is rejected. This is the load-bearing security
property of the whole system: **read `docs/vault.md`'s "Security boundary"
section before touching `release`, `refund`, or anything in
`authorize_and_pay`.** Any change there needs an accompanying test proving
a non-stream-contract caller is still rejected.

## How to add a new stream type

The current design supports one stream shape: linear accrual with an
optional cliff. To add a new shape (e.g. stepped/tranche vesting, or a
non-linear curve) without disturbing existing streams:

1. **Don't change `math::streamed_amount`'s signature or the existing
   `Stream` struct's meaning.** Existing streams must keep computing
   exactly as before.
2. Add a new variant to a stream "kind" concept — either a new field on
   `Stream` (e.g. `pub kind: StreamKind`, defaulting existing callers to
   `StreamKind::Linear`) or a parallel struct + storage key if the shape
   differs enough that most fields don't apply.
3. Add a new pure function in `math.rs` for the new curve (e.g.
   `stepped_streamed_amount`), and dispatch to it based on `kind` from
   `streamed_amount`/`withdrawable_amount`. Keep the function pure and
   `Env`-independent like the existing ones, so it can be unit tested the
   same way.
4. Write the same category of tests the linear implementation has: zero
   before start, full amount at/after end, an exact-fraction checkpoint,
   cliff gating, and overflow/precision edge cases.
5. `create_stream` (or a new `create_stream_with_kind`) validates the new
   parameters the same way it validates `start_ledger < end_ledger` today
   — fail loudly and specifically, never silently clamp bad input.
6. Update `docs/stream.md` with the new formula and a worked example, the
   same way the linear one is documented.

## Writing tests

**Naming.** Name each test `test_<function>_<behaviour>`, for example
`test_withdraw_rejects_non_recipient_caller`. The name should say what is
being checked, so a failing test is readable from the output alone.

**Typed errors.** Use the generated `try_*` client methods, such as
`try_create_stream`, when you need to assert on a specific erro

/* … truncated 2114 chars — edit only what you need near the top … */
