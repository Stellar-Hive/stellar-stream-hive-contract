# Stellar Stream Hive — Contracts

Soroban smart contracts for real-time, per-ledger token streaming on
Stellar. Think Sablier or Superfluid, native to Stellar: an employer
streams tokens to a recipient continuously between a start and end
ledger, and the recipient can withdraw whatever has accrued at any
moment. The primary use case is payroll, but the contracts are
general-purpose payment streams.

This repository holds the on-chain contracts only. See the sister repos
for the indexing API and the web frontend:

- API/indexer: https://github.com/Stellar-Hive/stellar-stream-hive-api
- Frontend: https://github.com/Stellar-Hive/stellar-stream-hive-frontend

## Architecture

Three contracts, each a workspace member under `contracts/`:

```
                     ┌────────────────────┐
   create_stream     │                    │   deposit / release / refund
   withdraw          │  Stream Contract   │──────────────────────────────┐
   cancel_stream      │  (math + rules)    │                              │
        │            └────────────────────┘                              ▼
        │                       ▲                                ┌───────────────┐
        │                       │ register_stream / update_stream│ Vault Contract │
        ▼                       │ (optional, off-chain wired)    │ (holds funds)  │
┌──────────────┐        ┌───────┴────────┐                       └───────────────┘
│ Employer /   │        │   Registry     │
│ Recipient    │        │   Contract     │
│ (end users)  │        │  (index/stats) │
└──────────────┘        └────────────────┘
```

- **[Stream](docs/stream.md)** is the core: it owns the linear, per-ledger
  accrual math, tracks each stream's schedule and withdrawal history, and
  enforces who can withdraw or cancel what. It never holds funds itself.
- **[Vault](docs/vault.md)** is the only contract that actually custodies
  tokens. It moves funds solely on instruction from the stream contract,
  gated by a hard-checked caller identity — see its
  ["security boundary"](docs/vault.md#the-security-boundary) section.
- **[Registry](docs/registry.md)** is a read-only index and aggregate
  stats layer (total streams, active streams, volume) for dashboards and
  block explorers. It never custodies funds and isn't in the trust path
  for any transfer.

## Quickstart

Requires Rust (via [rustup](https://rustup.rs)), the
`wasm32v1-none` target, and the
[Stellar CLI](https://developer.stellar.org/docs/tools/cli). Full setup
instructions are in [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
git clone https://github.com/Stellar-Hive/stellar-stream-hive-contract.git
cd stellar-stream-hive-contract

# Run the full test suite (30+ tests across all three contracts)
cargo test

# Build the real deployment artifacts
cargo build --target wasm32v1-none --release \
  -p stellar-stream-hive-vault \
  -p stellar-stream-hive-stream \
  -p stellar-stream-hive-registry

# Deploy to testnet
stellar keys generate deployer --network testnet --fund
scripts/deploy.sh testnet deployer
```

Deployed contract addresses are tracked in [DEPLOYMENTS.md](DEPLOYMENTS.md).

## How streaming works

```text
elapsed        = min(current_ledger, end_ledger) - start_ledger
total_duration = end_ledger - start_ledger
streamed       = total_amount * elapsed / total_duration
withdrawable   = streamed - withdrawn        (0 before cliff_ledger)
```

All of this runs on checked `i128` arithmetic, multiplying before
dividing to avoid precision loss, and is unit tested exhaustively in
`contracts/stream/src/math.rs`. See [docs/stream.md](docs/stream.md) for
the full write-up, a worked numeric example, and how cancellation freezes
a stream's accrued amount.

## Documentation

- [docs/stream.md](docs/stream.md) — the streaming contract: math,
  lifecycle, function reference, events, security notes.
- [docs/vault.md](docs/vault.md) — the vault contract: custody model, the
  security boundary, function reference, events.
- [docs/registry.md](docs/registry.md) — the registry contract: indexing
  model, stats, function reference, events.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for local dev setup, how to run
and extend the test suite, an explanation of the streaming math with a
worked example, the vault's security boundary, and how to add a new
stream type.

## License

[MIT](LICENSE)
