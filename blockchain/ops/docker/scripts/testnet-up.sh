#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="$DIR/../docker-compose.testnet.yml"

if [[ -z "${FAUCET_SK:-}" ]]; then
  echo "FAUCET_SK env var required (hex ed25519 key for faucet signer)" >&2
  exit 1
fi

docker compose -f "$COMPOSE_FILE" up -d --build
