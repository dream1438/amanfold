#!/bin/sh
# Same as cloud-init.yaml, for hosts without a cloud-config box (for example
# a Hostinger VPS). Run once as root on a fresh Ubuntu 24.04 server:
#   curl -fsSL https://raw.githubusercontent.com/dream1438/amanfold/main/deploy/scripts/prepare-host.sh | sh
set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get -y upgrade
apt-get -y install docker.io docker-compose-v2 git ufw fail2ban unattended-upgrades sqlite3

cat > /etc/ssh/sshd_config.d/10-amanfold.conf <<'CONF'
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin prohibit-password
MaxAuthTries 3
CONF
cat > /etc/apt/apt.conf.d/52amanfold-upgrades <<'CONF'
Unattended-Upgrade::Automatic-Reboot "true";
Unattended-Upgrade::Automatic-Reboot-Time "04:00";
CONF

# Refuse to lock you out: only disable passwords if a key is installed.
if [ ! -s /root/.ssh/authorized_keys ]; then
  rm /etc/ssh/sshd_config.d/10-amanfold.conf
  echo "WARNING: no SSH key in /root/.ssh/authorized_keys, so password login stays on. Add your key and run this again."
fi

ufw default deny incoming
ufw default allow outgoing
for p in 22/tcp 80/tcp 443/tcp 443/udp; do ufw allow $p; done
ufw --force enable
systemctl restart ssh
systemctl enable --now docker fail2ban unattended-upgrades

[ -d /opt/amanfold ] || git clone https://github.com/dream1438/amanfold.git /opt/amanfold
[ -f /opt/amanfold/deploy/.env ] || { cp /opt/amanfold/deploy/env.example /opt/amanfold/deploy/.env; chmod 600 /opt/amanfold/deploy/.env; }
echo "Host ready. Next: nano /opt/amanfold/deploy/.env"
