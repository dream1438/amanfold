#!/bin/sh
# Restore the database (and server keys) from a backup snapshot.
#   sudo sh scripts/restore.sh            # list snapshots
#   sudo sh scripts/restore.sh latest     # restore the newest one
set -eu
cd "$(dirname "$0")/.."
if [ $# -eq 0 ]; then
  docker compose run --rm --entrypoint restic backup snapshots
  exit 0
fi
snap=$1
echo "This stops the server and replaces its database with snapshot '$snap'."
printf 'Type RESTORE to continue: '; read -r ok; [ "$ok" = RESTORE ] || exit 1

docker compose stop server
docker compose run --rm --entrypoint sh backup -c "
  set -eu
  rm -rf /tmp/r && restic restore '$snap' --target /tmp/r
  db=\$(find /tmp/r -name amanfold.db | head -n1)
  [ \"\$(sqlite3 \"\$db\" 'PRAGMA integrity_check;')\" = ok ]
  mkdir -p /data/before-restore && mv /data/amanfold.db* /data/before-restore/ 2>/dev/null || true
  cp \"\$db\" /data/amanfold.db
  chown -R 65532:65532 /data
  find /tmp/r -name amanfold-secrets.json -exec cp {} /data/before-restore/restored-secrets.json \;
"
docker compose start server
echo "Restored. The old database is in the data volume under before-restore/."
echo "If the server keys were lost too, copy before-restore/restored-secrets.json over secrets/amanfold-secrets.json."
