#!/bin/sh
# Restore the database (and server keys) from a backup snapshot.
#   sudo sh scripts/restore.sh            # list snapshots
#   sudo sh scripts/restore.sh latest     # restore the newest one
#   sudo sh scripts/restore.sh 4bba301e   # restore a specific snapshot
set -eu
cd "$(dirname "$0")/.."
if [ $# -eq 0 ]; then
  docker compose run --rm --entrypoint restic backup snapshots
  exit 0
fi
snap=$1
case $snap in
  latest) ;;
  *[!0-9a-f]* | "") echo "Snapshot must be 'latest' or a hex snapshot ID."; exit 1 ;;
esac
echo "This stops the server and replaces its database with snapshot '$snap'."
printf 'Type RESTORE to continue: '; read -r ok; [ "$ok" = RESTORE ] || exit 1

# Whatever happens below, bring the server back up.
trap 'docker compose start server >/dev/null 2>&1 || true' EXIT
docker compose stop server

# The snapshot is passed as an argument, never pasted into the script.
docker compose run --rm --cap-add CHOWN --cap-add FOWNER --entrypoint sh backup -c '
  set -eu
  snap=$1
  rm -rf /tmp/r
  restic restore "$snap" --target /tmp/r
  db=$(find /tmp/r -name amanfold.db -type f | head -n1)
  if [ -z "$db" ] || [ ! -s "$db" ]; then
    echo "restore: snapshot has no database; nothing changed." >&2
    exit 1
  fi
  [ "$(sqlite3 "$db" "PRAGMA integrity_check;")" = ok ] || { echo "restore: database in snapshot is damaged; nothing changed." >&2; exit 1; }
  old=/data/before-restore-$(date -u +%Y%m%dT%H%M%SZ)
  mkdir -p "$old"
  for f in /data/amanfold.db /data/amanfold.db-wal /data/amanfold.db-shm; do
    if [ -e "$f" ]; then mv "$f" "$old/"; fi
  done
  cp "$db" /data/amanfold.db
  keys=$(find /tmp/r -name amanfold-secrets.json -type f | head -n1)
  if [ -n "$keys" ]; then cp "$keys" "$old/restored-secrets.json"; fi
  chown -R 65532:65532 /data/amanfold.db "$old"
  echo "restore: done; previous database kept in $old"
' _ "$snap"

docker compose start server
trap - EXIT
echo "Restored. If the server keys were lost too, copy restored-secrets.json from"
echo "that before-restore folder (in the data volume) over secrets/amanfold-secrets.json."
