#!/bin/sh
# Pull the latest deploy files and server image, then restart what changed.
# Takes a backup first so a bad upgrade can be rolled back.
set -eu
cd "$(dirname "$0")/.."
git pull --ff-only
docker compose exec -T backup /usr/local/bin/backup.sh || echo "warning: pre-update backup failed"
docker compose pull server caddy
docker compose build --pull backup
docker compose up -d --remove-orphans
docker image prune -f >/dev/null
docker compose ps
