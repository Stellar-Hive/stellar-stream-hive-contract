# Lists the available recipes.
default:
    @just --list

# Runs every test in the workspace.
test:
    cargo test --workspace

# Formats the code.
fmt:
    cargo fmt --all

# Checks formatting and runs clippy with the same flags as CI.
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings -A clippy::too_many_arguments -A clippy::needless_borrows_for_generic_args

# Builds every contract to the deployment WASM target.
wasm:
    cargo build --target wasm32v1-none --release -p stellar-stream-hive-vault -p stellar-stream-hive-stream -p stellar-stream-hive-registry

# Runs everything CI runs.
check: lint test wasm
