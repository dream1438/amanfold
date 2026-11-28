#!/bin/sh
set -eu
echo "${BACKUP_SCHEDULE:-17 * * * *} /usr/local/bin/backup.sh >> /proc/1/fd/1 2>&1" > /etc/crontabs/root
echo "backup: schedule '${BACKUP_SCHEDULE:-17 * * * *}', repository ${RESTIC_REPOSITORY%%:*}:..."
exec crond -f -l 6
