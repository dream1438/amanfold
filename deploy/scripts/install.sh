#!/bin/sh
# First-time setup on the server. Run from the deploy/ folder:
#   sudo sh scripts/install.sh
# Safe to run again: it never replaces existing secrets.
set -eu
cd "$(dirname "$0")/.."

[ -f .env ] || { cp env.example .env; chmod 600 .env; echo "Created .env. Fill it in (nano .env), then run this again."; exit 1; }
setting() { sed -n "s/^$1=//p" .env | tail -n1; }
for v in ACME_EMAIL AMANFOLD_SMTP_URL AMANFOLD_MAIL_FROM RESTIC_REPOSITORY; do
  val=$(setting $v)
  [ -n "$val" ] && [ "$val" != "you@example.com" ] || { echo "Set $v in .env first."; exit 1; }
done

mkdir -p secrets/ssh
chmod 700 secrets

if [ ! -s secrets/restic-password ]; then
  head -c 32 /dev/urandom | base64 | tr -d '\n=' > secrets/restic-password
  chmod 400 secrets/restic-password
  echo
  echo "Backup password (save it in your password manager NOW; without it backups cannot be restored):"
  cat secrets/restic-password; echo
fi

if [ ! -s secrets/amanfold-secrets.json ]; then
  image=$(setting AMANFOLD_IMAGE); image=${image:-ghcr.io/dream1438/amanfold-server:latest}
  docker image inspect "$image" >/dev/null 2>&1 || docker pull "$image" || docker build -f server.Dockerfile -t "$image" ..
  docker run --rm "$image" keygen > secrets/amanfold-secrets.json 2> secrets/opaque-public-key.txt
  echo
  echo "Server keys created. $(cat secrets/opaque-public-key.txt)"
  echo "Send that public key to Claude to pin in the apps. It is not secret."
fi
# The server runs as uid 65532 and must be able to read its key file.
chown 65532:65532 secrets/amanfold-secrets.json
chmod 400 secrets/amanfold-secrets.json

case $(setting RESTIC_REPOSITORY) in sftp:*)
if [ ! -f secrets/ssh/id_ed25519 ]; then
  ssh-keygen -q -t ed25519 -N '' -C amanfold-backup -f secrets/ssh/id_ed25519
  printf 'Host *.your-storagebox.de\n  Port 23\n  StrictHostKeyChecking accept-new\n' > secrets/ssh/config
  chmod 600 secrets/ssh/config secrets/ssh/id_ed25519
  echo
  echo "Backup SSH key created. Add it to the Storage Box (Hetzner console > Storage Box > SSH keys):"
  cat secrets/ssh/id_ed25519.pub
fi
;; esac

docker compose build backup
docker compose up -d
echo
echo "Started. Check: curl -s https://$(setting API_DOMAIN)/v1/server"
