## Summary

<!-- What changes, and why? -->

## Checklist

- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --all -- --check` and the clippy command in CI are clean (`just lint` runs both).
- [ ] The three contracts still build for `wasm32v1-none` (`just wasm`).
- [ ] New public contract functions have tests for the happy path and at least one rejection path.
- [ ] If the math changed, `docs/stream.md` is updated in this PR.

## Security-sensitive changes

If this PR touches the vault's `release`, `refund` or `authorize_and_pay`, or anything that decides who can call them:

- [ ] I read the "Security boundary" section of `docs/vault.md`.
- [ ] A test proves that a caller other than the configured stream contract is still rejected.

Report vulnerabilities privately through [SECURITY.md](../SECURITY.md), not in a PR.
