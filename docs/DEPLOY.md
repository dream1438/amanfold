# Deploying the Amanfold server

This puts the sync server ([SERVER.md](SERVER.md)) and the website live on
`api.amanfold.com` and `amanfold.com`, on one small Linux server.

> **Beta, not independently audited.** See [SECURITY.md](SECURITY.md).

## What runs where

```text
           internet
              │  443 (HTTPS, HTTP/3)   80 (redirect + certificate checks)
        ┌─────▼──────────────────────────────────────────────┐
        │ Caddy   api.amanfold.com → server:8080              │
        │         amanfold.com     → site/ (static files)     │
        │         www.amanfold.com → redirect to amanfold.com │
        ├────────────────────────────────────────────────────┤
        │ amanfold-server   (port 8080, not published)        │──► SMTP (Resend)
        │   /data/amanfold.db  (SQLite, ciphertext + emails)  │
        ├────────────────────────────────────────────────────┤
        │ backup   hourly snapshot → restic (encrypted) ──────│──► Backblaze B2
        └────────────────────────────────────────────────────┘    or Storage Box
```

Everything is in [`deploy/`](../deploy):

| File | |
| --- | --- |
| `compose.yaml` | The three services. The server container is read-only, has no Linux capabilities and is reachable only from Caddy. |
| `server.Dockerfile` | Builds the server into a distroless image (no shell, runs as uid 65532). CI publishes it as `ghcr.io/dream1438/amanfold-server`. |
| `Caddyfile` | Automatic Let's Encrypt certificates, HSTS, strict CSP. **No access log**: we keep no record of who connects. |
| `backup/` | Hourly `sqlite3 .backup`, integrity check, `restic` upload (encrypted before it leaves the server), retention 48 hourly / 30 daily / 12 weekly / 24 monthly, daily read-back check, optional health-check ping. |
| `env.example` | Every setting, with examples. Copied to `.env` on the server. |
| `cloud-init.yaml` | First-boot setup for Hetzner or DigitalOcean: SSH keys only, firewall (22, 80, 443), automatic security updates, fail2ban, Docker. |
| `scripts/prepare-host.sh` | The same as `cloud-init.yaml`, for hosts without a cloud-config box (Hostinger VPS). |
| `scripts/install.sh` | First start: creates the server keys and backup password, then starts everything. Never overwrites existing secrets. |
| `scripts/update.sh` | Backup, pull the new image, restart. |
| `scripts/restore.sh` | List snapshots, or restore one. |

The workflow `.github/workflows/server-image.yml` builds the images and runs
the whole stack on every change (API and site through Caddy, a real backup
and snapshot listing), then publishes the server image from `main`.

## Monthly cost (beta)

| | Choice | Cost |
| --- | --- | --- |
| Server | Hostinger VPS KVM 1 (1 vCPU, 4 GB, 50 GB) or Hetzner Cloud CX23 (2 vCPU, 4 GB, 40 GB) | about $5–7 at first (Hostinger, higher on renewal) or €4–5 (Hetzner) |
| Email | Resend free tier (3,000 emails/month, 100/day) | €0, then $20 for 50,000 |
| Backups | Backblaze B2 (a few GB) or Hetzner Storage Box BX11 (1 TB) | under €0.10, or about €3.20 |
| Certificates | Let's Encrypt via Caddy | €0 |
| Monitoring | UptimeRobot + healthchecks.io free plans | €0 |
| Domain | amanfold.com at Hostinger (already paid) | — |

## Step by step

Steps marked **you** need your own accounts or payment. Never paste a
password, API key or the server's secrets into a chat; they go only into the
provider's website or the server's `.env` file.

### 1. Create an SSH key (you, once, on your computer)

macOS / Linux terminal, or Windows PowerShell:

```sh
ssh-keygen -t ed25519 -C "eslam-amanfold"
```

Press Enter to accept the location and choose a passphrase. Your **public**
key is in `~/.ssh/id_ed25519.pub`; that one is safe to paste into the host's
website.

### 2. Create the server (you)

**Hetzner Cloud** (recommended): console.hetzner.cloud → New project
"Amanfold" → Add server.

* Location: Falkenstein or Nuremberg (Germany), or Helsinki.
* Image: **Ubuntu 26.04 LTS** (24.04 LTS also works). Type: **Shared vCPU, x86, CX23**.
* Networking: public IPv4 and IPv6.
* SSH keys: add the public key from step 1.
* Backups: optional (+20%); our own off-site backups already cover the data.
* Cloud config: paste the whole of [`deploy/cloud-init.yaml`](../deploy/cloud-init.yaml).
* Name: `amanfold-1`. Create, and note the IPv4 and IPv6 addresses.

Also add a Hetzner **Firewall** allowing inbound TCP 22, 80, 443 and UDP 443
only, applied to the server.

**DigitalOcean**: the same, with a Basic $6 Droplet, Ubuntu 26.04 LTS, your SSH
key, and `cloud-init.yaml` under *Advanced options → Add initialization scripts*.

**Hostinger VPS** (same account as the domain):

1. hostinger.com → **VPS** → choose **KVM 1** (1 vCPU, 4 GB RAM, 50 GB
   disk), which is plenty for the beta. KVM 2 only if you expect thousands
   of active users. A 12- or 24-month term keeps the price low; check the
   renewal price on the checkout page.
2. During setup:
   * **Location**: a European data center (Netherlands, Germany, France,
     Lithuania or the UK, whichever is offered). Europe is a good middle
     point for users in the Middle East, Africa and Europe, and EU privacy
     law suits a password manager.
   * **Operating system**: *Plain OS* → **Ubuntu 26.04 LTS** (24.04 LTS also works). Not a control
     panel, not "Docker" or "Coolify" templates, and no "application".
   * **Root password**: let your password manager generate a long one and
     save it there. You will rarely need it.
   * **SSH key**: paste the public key from step 1 (or later under
     *VPS → Settings → SSH keys*).
   * Malware scanner (Monarx): fine to leave on.
3. hPanel → **VPS → Security → Firewall** → create a firewall named
   `amanfold` with *accept* rules for TCP 22, TCP 80, TCP 443 and UDP 443
   (everything else is dropped), and **activate** it for this VPS.
4. Note the VPS's **IPv4** and **IPv6** addresses on the VPS overview page.
5. Log in: on your computer, `ssh root@<IPv4>`. Or, without any setup,
   use **VPS → Browser terminal** in hPanel. Then run:

   ```sh
   curl -fsSL https://raw.githubusercontent.com/dream1438/amanfold/main/deploy/scripts/prepare-host.sh | sh
   ```

   It turns on the server's own firewall, automatic security updates and
   fail2ban, installs Docker, and fetches the setup files to
   `/opt/amanfold`. If your SSH key is installed it also turns off password
   logins.
6. Optional: *VPS → Snapshots & backups* lets you take a snapshot before a
   big change. Our own hourly backups (step 6) are what protect the data.

Then continue with step 3. Your domain is already at Hostinger, so the DNS
records go in the same hPanel.

### 3. Point the domain at the server (you, at Hostinger)

hPanel → **Domains** → amanfold.com → **DNS / Nameservers** → *DNS records*
(the domain must be using Hostinger's nameservers, which is the default).
Delete the existing `A`, `AAAA` and `CNAME` records for `@`, `www` and `api`
(Hostinger's parking page), then add:

| Type | Name | Points to / Value | TTL |
| --- | --- | --- | --- |
| A | `api` | server IPv4 | 3600 |
| AAAA | `api` | server IPv6 | 3600 |
| A | `@` | server IPv4 | 3600 |
| AAAA | `@` | server IPv6 | 3600 |
| CNAME | `www` | `amanfold.com` | 3600 |
| CAA | `@` | `0 issue "letsencrypt.org"` | 3600 |
| CAA | `@` | `0 issue "sectigo.com"` | 3600 |
| CAA | `@` | `0 iodef "mailto:<your email>"` | 3600 |

The CAA records mean only Let's Encrypt (and ZeroSSL, Caddy's fallback, which
uses Sectigo) may issue certificates for amanfold.com: a second line of
defence against a forged certificate for the API.

Check after a few minutes: `nslookup api.amanfold.com` shows the server IP.

### 4. Log in to the server

```sh
ssh root@<server IPv4>
```

### 5. Set up email (you, at Resend)

resend.com → sign up → **Domains → Add domain** `amanfold.com`, region
**EU (Ireland)** if the server is in Europe. Resend shows about four records;
add each one in Hostinger DNS exactly as shown (they look like this):

| Type | Name | Value |
| --- | --- | --- |
| TXT | `resend._domainkey` | `p=MIGfMA0G…` (DKIM key, copy from Resend) |
| MX | `send` | `feedback-smtp.eu-west-1.amazonses.com`, priority 10 |
| TXT | `send` | `v=spf1 include:amazonses.com ~all` |

Then add a DMARC policy so nobody else can send mail as amanfold.com (this
matters for a password manager, since phishers love to impersonate one):

| Type | Name | Value |
| --- | --- | --- |
| TXT | `_dmarc` | `v=DMARC1; p=quarantine; adkim=r; aspf=r` |

Click **Verify** in Resend. Then **API Keys → Create**, permission *Sending
access*, domain amanfold.com. Copy the key straight into `.env` in step 7.

If you later add mailboxes at amanfold.com (for example Hostinger Email for
`support@`), add that provider's MX, SPF and DKIM records too; the ones
above do not conflict with them.

### 6. Set up backup storage (you)

**Backblaze B2**: backblaze.com → B2 Cloud Storage → **Create bucket**
`amanfold-backups` (private, *Object Lock* off, region EU Central). Then
**Application Keys → Add** with access to that bucket only, *Read and Write*.
Note the endpoint shown on the bucket (for example
`s3.eu-central-003.backblazeb2.com`), the keyID and the applicationKey.

**Or Hetzner Storage Box**: order a BX11, enable *SSH support*. The install
script prints a backup SSH key to add under the Storage Box's SSH keys.

Optional: healthchecks.io → new check, period 1 hour, grace 2 hours. Copy
its ping URL. You get an email if backups stop.

### 7. Fill in the settings (on the server)

```sh
cd /opt/amanfold/deploy
nano .env
```

Fill in `ACME_EMAIL` (your email), `AMANFOLD_SMTP_URL` as
`smtps://resend:<Resend API key>@smtp.resend.com:465`, the backup settings
from step 6 (`RESTIC_REPOSITORY` with your B2 endpoint and bucket,
`AWS_ACCESS_KEY_ID` = keyID, `AWS_SECRET_ACCESS_KEY` = applicationKey) and
`BACKUP_PING_URL`. Save with Ctrl+O, Enter, Ctrl+X.

### 8. Start (on the server)

```sh
sh scripts/install.sh
```

It prints two things:

* **The backup password.** Save it in your password manager now, and print
  it with your recovery kit. Without it the backups cannot be restored.
* **The OPAQUE public key.** Not secret. Send it to Claude: it gets pinned
  in the browser extension and the mobile apps, so they refuse an impostor
  server.

Check it works:

```sh
curl -s https://api.amanfold.com/v1/server
docker compose ps
docker compose exec backup /usr/local/bin/backup.sh   # first backup now
```

Also keep an offline copy of `secrets/amanfold-secrets.json` (for example as
a secure note in your password manager). It is in every backup, but if both
the server and the backup password were lost, every user would have to
re-pin a new server key.

### 9. Monitoring (you)

uptimerobot.com → **New monitor**, HTTPS, URL
`https://api.amanfold.com/v1/server`, every 5 minutes, keyword
`opaque_public_key`. Add a second for `https://amanfold.com`. You get an
email when either goes down. UptimeRobot also warns before a certificate
expires (Caddy renews 30 days early, so a warning means something is wrong).

### 10. Make the server image public (you, once)

After the first merge to `main`, GitHub → your profile → **Packages** →
`amanfold-server` → *Package settings* → **Change visibility → Public**.
Until then the server builds the image itself (about 10 minutes).

## Routine work

| Task | Command (in `/opt/amanfold/deploy`) |
| --- | --- |
| Update | `sh scripts/update.sh` |
| Logs | `docker compose logs -f server` |
| Status | `docker compose ps` |
| Backups | `docker compose exec backup restic snapshots` |
| Restore | `sh scripts/restore.sh` (list), `sh scripts/restore.sh latest` |
| Pin an exact version | set `AMANFOLD_IMAGE=ghcr.io/dream1438/amanfold-server:sha-abc1234` in `.env`, then `docker compose up -d` |

Ubuntu installs security updates every night and reboots at 04:00 server
time when a kernel update needs it; the containers come back on their own.

Test a restore at least once a month on a throwaway server: create a new
server from step 2, copy `.env` and `secrets/restic-password`, run
`sh scripts/restore.sh latest`, and sign in with a test account.

## What a stolen server would reveal

The server holds ciphertext only (see SERVER.md). Someone who copies the
whole machine gets: account email addresses, public keys, vault sizes and
sharing relationships, and the server's own key (letting them impersonate the
server to apps that have not pinned it, but not read any vault). They do not
get master passwords, Secret Keys, vault keys or any item. Backups contain the
same and are additionally encrypted with the restic password.

## Not covered yet

* **One server.** If it fails, restore from backup onto a new one (under an
  hour of data loss, because backups are hourly). High availability needs
  Postgres and a shared rate-limit store (SERVER.md, *Limits*).
* **No status page or alerting by SMS**; email alerts only.
* **`AMANFOLD_ALLOWED_ORIGINS`** is passed through for the website's waitlist
  form; it takes effect once the server supports it.
