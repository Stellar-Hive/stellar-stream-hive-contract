# Security Policy

These contracts custody user funds (through the vault), so security reports
are taken seriously.

## Reporting a vulnerability

Please **do not open a public issue** for a suspected vulnerability. Use
GitHub's private reporting instead:
**Security -> Report a vulnerability** on this repository.

Include the affected contract and function, a description of the impact, and
ideally a failing test that demonstrates it. We aim to acknowledge reports
within 72 hours.

## Scope

In scope: `contracts/vault`, `contracts/stream`, `contracts/registry`, and
the deployment script. Of particular interest:

- anything that lets a caller other than the configured stream contract move
  vault funds (see the vault's "security boundary" in `docs/vault.md`),
- any way for a recipient to withdraw more than has accrued, or for the sum of
  payouts and refunds to exceed a stream's deposit,
- anything that can trap funds in a stream (the `pause` switch is designed
  never to).

## Supported versions

Security fixes land on `main` and only on `main`. There are no release
branches and no tagged releases, so "the latest release" always means
current `main`; the `0.1.0` entry in `CHANGELOG.md` marks the initial
contracts, not a separately maintained version line.

| Version / deployment | Supported |
|---|---|
| `main` | Yes — the only line that receives fixes |
| Testnet deployment of 2026-10-12 (see [`DEPLOYMENTS.md`](DEPLOYMENTS.md)) | Built from `main` at that date. Fixes are **not** applied on-chain retroactively — redeploy from the fixed `main` to pick them up. |
| Mainnet | Not deployed. Mainnet deployment will only follow a successful testnet deployment, a completed audit, and explicit sign-off (see [`DEPLOYMENTS.md`](DEPLOYMENTS.md)). |

## Status

The contracts have **not** been externally audited. Do not use them with
mainnet funds you cannot afford to lose.
