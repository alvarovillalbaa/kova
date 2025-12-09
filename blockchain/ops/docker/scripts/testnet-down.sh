#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="$DIR/../docker-compose.testnet.yml"

docker compose -f "$COMPOSE_FILE" down -v
