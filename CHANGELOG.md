# Changelog

All notable changes to the Stellar Stream Hive contracts. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed
- **Stream (funds stranded on early cancel):** cancelling a stream *before its
  cliff* refunded the sender only the unstreamed remainder, yet
  `withdrawable_amount` returned 0 forever for such a stream, so the
  accrued-but-unclaimable part stayed locked in the vault permanently. A
  pre-cliff cancel now entitles the recipient to nothing and refunds the sender
  in full; `cancel` events report `streamed_now = 0` in that case. Found by the
  new randomized conservation test.

### Added
- **Stream:** `pause` / `unpause` / `is_paused`. Pausing blocks only
  `create_stream`; withdrawals and cancellations always keep working so
  funds are never trapped.
- **Stream:** `transfer_admin`, requiring authorization from both the old and
  new admin.
- **Stream:** `count_streams_by_sender|recipient` and paginated
  `get_streams_by_sender|recipient_page` (page size capped at 50) so
  addresses with many streams stay within Soroban resource limits.
- **Stream:** new `Paused` error (code 13) and `Pause` / `Unpause` /
  `AdminTransfer` events.
- Math invariant tests: monotonic accrual, withdrawals never exceed the
  deposit, rounding never favours the recipient, cancel-before-cliff pays
  nothing.
- CI: `cargo fmt --check` and clippy (`-D warnings`) job.
- `SECURITY.md` with a private disclosure process.

### Changed
- **Toolchain / CI:** the pinned Rust version in `rust-toolchain.toml` moves
  from 1.85.0 to 1.91.0 — the floor the locked dependencies require
  (`soroban-sdk` 27.0.6 declares `rust-version = 1.91.0`) — and the file now
  also declares the `rustfmt` and `clippy` components; `ci.yml` mirrors the
  new version. Until this, every CI job on `main` failed before running a
  single check: rustup's auto-install of the pinned toolchain omitted
  `cargo-fmt`, and cargo refused to build the lockfile on 1.85.0. Test code
  left unformatted by recent merges is reformatted so
  `cargo fmt --all -- --check` is clean again.
- **Deploy script:** validates the network and identity up front, requires
  `CONFIRM_MAINNET=yes` for mainnet, verifies the deployed wiring on-chain,
  and writes `deployments/<network>.json` with WASM hashes plus ready-to-paste
  env lines for the frontend and API. Shell scripts are pinned to LF via
  `.gitattributes` so Windows checkouts do not break them.
- **Registry:** stream ids are now stored one ledger entry per registration
  index (`StreamCount` + `StreamIdAt`) instead of a single growing `Vec`, so
  registration stays O(1) and cannot hit the ledger-entry size limit. Added
  `stream_count` and `get_streams_page` (page size capped at 50). Storage
  layout change: only safe before a first deployment.
- **Stream:** the `create` event now also carries `cliff_ledger` and
  `cancellable`, so an indexer can rebuild a stream from events alone.
  **Breaking for event consumers** -- the API repo is updated to match.

## [0.1.0]
- Initial vault, stream and registry contracts.
