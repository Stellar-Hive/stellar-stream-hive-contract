# Documentation Index

Welcome to the Stellar Stream Hive contract documentation. This directory contains architectural specifications, CLI guides, contract references, and storage layouts.

## Reading Order for New Contributors

1. **[Architecture](stream.md)**: Start with the core streaming model and mathematical formulas for linear token accrual.
2. **[Vault Contract](vault.md)**: Understand custody, authentication separation, and locked balance management.
3. **[Registry Contract](registry.md)**: Explore the global stream directory and index structures.
4. **[Events & Telemetry](events.md)**: See how on-chain contract events are structured and decoded from Soroban RPC.
5. **[CLI Reference](cli.md)**: Hands-on examples for invoking and querying contracts using the `stellar` CLI.

---

## Documents Directory

| Document | Description |
| :--- | :--- |
| **[stream.md](stream.md)** | Core linear streaming contract specification: stream terms, lifecycle states, withdrawal mechanics, cancellation rules, and admin emergency pause controls. |
| **[vault.md](vault.md)** | Vault contract specification: secure fund custody, caller authentication guarantees, token accounting, and release/refund flows. |
| **[registry.md](registry.md)** | Stream registry specification: contract registration indexing, pagination helpers, and network-wide streaming statistics. |
| **[events.md](events.md)** | Detailed event schema across contracts and instructions for fetching and decoding events from Soroban RPC endpoints. |
| **[cli.md](cli.md)** | Comprehensive CLI examples for deployment, contract initialization, stream management, withdrawals, and administrative operations. |
