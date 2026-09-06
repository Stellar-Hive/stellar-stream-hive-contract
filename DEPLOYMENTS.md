# Deployments

This file records the on-chain addresses of every deployed Stellar Stream
Hive contract, per network.

## Testnet

_Deployment pending — run `scripts/deploy.sh` once the Stellar CLI and a
funded identity are available. Contract IDs will be recorded here._

| Contract | Contract ID | WASM hash | Deployed at |
|---|---|---|---|
| Vault | _pending_ | _pending_ | _pending_ |
| Stream | _pending_ | _pending_ | _pending_ |
| Registry | _pending_ | _pending_ | _pending_ |

## Mainnet

_Not yet deployed. Mainnet deployment will only follow a successful
testnet deployment, a completed audit, and explicit sign-off — this
project is a grant-stage prototype._

## How this file gets updated

`scripts/deploy.sh` builds, optimizes, and deploys all three contracts in
dependency order (vault → stream → registry) and prints the resulting
contract IDs. After running it, update the tables above with the network,
contract IDs, the WASM hash (`stellar contract info` or the hash printed
during deploy), and the deployment date.
