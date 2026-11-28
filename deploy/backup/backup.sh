#!/bin/sh
# Takes a consistent SQLite snapshot, uploads it with restic (encrypted
# client-side with the restic password), prunes old snapshots, and pings an
# optional health-check URL so a silent failure gets noticed.
set -eu
ping() { [ -n "${BACKUP_PING_URL:-}" ] && curl -fsS -m 10 --retry 3 "${BACKUP_PING_URL}$1" >/dev/null || true; }
# Nothing to back up until the server has created its database (and never
# let sqlite3 create it here: it would be owned by the wrong user).
[ -f /data/amanfold.db ] || { echo "backup: no database yet"; exit 0; }

work=$(mktemp -d)
trap 'rm -rf "$work"; ping /fail' EXIT
ping /start

sqlite3 -readonly /data/amanfold.db ".backup '$work/amanfold.db'"
[ "$(sqlite3 "$work/amanfold.db" 'PRAGMA integrity_check;')" = "ok" ]
cp /run/secrets/amanfold "$work/amanfold-secrets.json"

restic snapshots >/dev/null 2>&1 || restic init
restic backup --host amanfold --tag hourly "$work"
restic forget --host amanfold --keep-hourly 48 --keep-daily 30 --keep-weekly 12 --keep-monthly 24 --prune
# Read back a sample of the data every day at 03:xx.
[ "$(date +%H)" = "03" ] && restic check --read-data-subset=5%

rm -rf "$work"
trap - EXIT
ping ""
echo "backup: ok $(date -u +%FT%TZ)"
