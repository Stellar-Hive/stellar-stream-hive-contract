# Architecture Overview: Contract Interactions

This document explains how the three Stellar Stream Hive smart contracts (`Stream`, `Vault`, and `Registry`) interact across key protocol workflows.

```
       +------------------------------------+
       |          Client / Frontend         |
       +-----------------+------------------+
                         |
           create_stream | withdraw / cancel
                         v
                +-----------------+
                | Stream Contract |
                +--------+--------+
                         |
      authorize_and_pay  |  release / refund
                         v
                 +---------------+
                 | Vault Contract|
                 +-------+-------+
                         |
             transfer SAC|
                         v
               +-------------------+
               | Token SAC Contract|
               +-------------------+
```

---

## 1. Stream Creation & Escrow Deposit

When a sender creates a stream, tokens are pulled into the Vault before state is committed in the Stream contract.

```mermaid
sequenceDiagram
    autonumber
    actor Sender
    participant Stream as Stream Contract
    participant Vault as Vault Contract
    participant Token as Stellar Asset Contract (SAC)
    participant Registry as Registry Contract

    Sender->>Stream: create_stream(recipient, token, amount, start, end, cliff, cancellable)
    Note over Stream: Verify parameters, check pause status, bump TTL
    Stream->>Vault: deposit(sender, token, amount, stream_id)
    Vault->>Token: transfer(sender, vault_address, amount)
    Token-->>Vault: Ok
    Vault-->>Stream: Ok (StreamBalance credited)
    Stream->>Stream: store Stream record
    opt Registry configured
        Stream->>Registry: register_stream(stream_id, sender, recipient, ...)
    end
    Stream-->>Sender: stream_id
```

---

## 2. Accrued Token Withdrawal

The recipient claims tokens accrued up to the current ledger sequence.

```mermaid
sequenceDiagram
    autonumber
    actor Recipient
    participant Stream as Stream Contract
    participant Vault as Vault Contract
    participant Token as Stellar Asset Contract (SAC)

    Recipient->>Stream: withdraw(stream_id, amount)
    Note over Stream: Calculate streamed_amount(current_ledger) - withdrawn
    Stream->>Vault: release(recipient, stream_id, amount)
    Vault->>Vault: require_auth(stream_contract)
    Vault->>Vault: decrement StreamBalance(stream_id)
    Vault->>Token: transfer(vault_address, recipient, amount)
    Token-->>Vault: Ok
    Vault-->>Stream: Ok
    Stream->>Stream: update withdrawn amount & bump TTL
    Stream-->>Recipient: Ok
```

---

## 3. Stream Cancellation & Fund Distribution

If a cancellable stream is cancelled, accrued funds are sent to the recipient and remainder is refunded to the sender.

```mermaid
sequenceDiagram
    autonumber
    actor SenderOrRecipient as Sender / Recipient
    participant Stream as Stream Contract
    participant Vault as Vault Contract
    participant Token as Stellar Asset Contract (SAC)

    SenderOrRecipient->>Stream: cancel_stream(stream_id)
    Note over Stream: Verify cancellable flag & auth
    Stream->>Vault: release(recipient, stream_id, recipient_due)
    Vault->>Token: transfer(vault_address, recipient, recipient_due)
    Stream->>Vault: refund(sender, stream_id, sender_refund)
    Vault->>Token: transfer(vault_address, sender, sender_refund)
    Stream->>Stream: mark StreamStatus::Cancelled
    Stream-->>SenderOrRecipient: Ok
```
