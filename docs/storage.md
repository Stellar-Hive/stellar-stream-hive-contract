# Contract Storage Layout & DataKey Reference

This document provides a comprehensive specification of every storage key, value type, storage tier (instance vs persistent), and TTL management policy across the three Stellar Stream Hive contracts.

## Storage Tiers & TTL Policies

Soroban smart contracts use two primary on-chain storage tiers:

1. **Instance Storage**:
   - Single shared ledger entry for the entire contract instance.
   - Holds contract-wide state: admin address, configuration addresses, global counters, and pause status.
   - Shared TTL: extending the instance entry keeps all instance keys alive.
   - Threshold: `120,960` ledgers (~7 days at 5s/ledger).
   - Bump target: `241,920` ledgers (~14 days).

2. **Persistent Storage**:
   - Separate ledger entries per key.
   - Holds unbounded and per-stream records (streams, balances, index lists).
   - Independent TTLs per record.

---

## 1. Stream Contract (`contracts/stream/src/storage.rs`)

| DataKey Variant | Storage Tier | Value Type | Written By | Description & TTL Policy |
| :--- | :--- | :--- | :--- | :--- |
| `Admin` | Instance | `Address` | `initialize` | Contract admin with privilege to pause/unpause. |
| `Vault` | Instance | `Address` | `initialize` | Configured Vault contract holding escrowed stream funds. |
| `NextStreamId` | Instance | `u64` | `initialize`, `create_stream` | Monotonically increasing stream sequence identifier. |
| `Paused` | Instance | `bool` | `initialize`, `pause`, `unpause` | Circuit-breaker switch preventing new stream creations. |
| `Stream(u64)` | Persistent | `Stream` struct | `create_stream`, `withdraw`, `cancel_stream` | Stream configuration, terms, status, and withdrawn amounts. Bumps TTL on write. |
| `SenderStreams(Address)` | Persistent | `Vec<u64>` | `create_stream` | Index list of stream IDs initiated by a sender. Bumps TTL on append. |
| `RecipientStreams(Address)` | Persistent | `Vec<u64>` | `create_stream` | Index list of stream IDs payable to a recipient. Bumps TTL on append. |

---

## 2. Vault Contract (`contracts/vault/src/storage.rs`)

| DataKey Variant | Storage Tier | Value Type | Written By | Description & TTL Policy |
| :--- | :--- | :--- | :--- | :--- |
| `Admin` | Instance | `Address` | `initialize` | Vault administrator address. |
| `StreamContract` | Instance | `Address` | `initialize` | Authorized stream contract address permitted to release/refund funds. |
| `StreamBalance(u64)` | Persistent | `i128` | `deposit`, `release`, `refund` | Escrow balance remaining for a specific stream ID. Bumps TTL on update. |
| `StreamToken(u64)` | Persistent | `Address` | `deposit` | Asset SAC / token address deposited for the stream. |
| `TotalLocked(Address)` | Persistent | `i128` | `deposit`, `release`, `refund` | Aggregated total locked amount for a given token across all streams. |

---

## 3. Registry Contract (`contracts/registry/src/storage.rs`)

| DataKey Variant | Storage Tier | Value Type | Written By | Description & TTL Policy |
| :--- | :--- | :--- | :--- | :--- |
| `Admin` | Instance | `Address` | `initialize` | Registry contract administrator. |
| `StreamCount` | Instance | `u32` | `initialize`, `register_stream` | Total number of registered stream records in the global index. |
| `StreamIdAt(u32)` | Persistent | `u64` | `register_stream` | Maps sequential registry index to stream ID. |
| `Record(u64)` | Persistent | `StreamRecord` | `register_stream`, `update_status` | Metadata record snapshot for fast querying. |
| `Stats` | Instance | `RegistryStats` | `initialize`, `register_stream`, `update_status` | Aggregated statistics: total streams, active streams, cancelled streams. |
