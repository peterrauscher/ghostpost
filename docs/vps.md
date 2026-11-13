# Production VPS runbook (OVH)

The production backend, worker and PostgreSQL run on one OVH VPS:

- Login: SSH to `GHOSTPOST_HOST` (no default; set it to `user@host`).
- OS: Debian. Docker Engine and the Compose plugin run the stack.

The concrete IPv4, IPv6 and provider hostname are kept in the operator's
private notes, not in this repository.

This runbook covers the VPS only. The topology, R2 buckets, WorkOS settings and
smoke checks are in [the deployment plan](deployment-plan.md). The HTPC libvirt
VM that served production before is documented only in the
[legacy source appendix](#appendix-legacy-htpc-source-recovery).

Do not assume the VPS CPU, RAM or disk size. Read them from the OVH panel and
from `nproc`, `free -h` and `df -h /var/lib/docker` on the host, and record the
values in your own notes before the first build.

## What stays outside the VPS

Nothing here moves and nothing on this page edits it:

- Cloudflare Workers static assets for `app.getghostpost.com` and
  `getghostpost.com`, and the landing D1 waitlist.
- R2 buckets: archive uploads (`ghostpost-archives`, including the
  `deletion-ledger/v1/` prefix) and `ghostpost-backups`.
- WorkOS redirect URIs and webhook URL. They keep pointing at
  `https://app.getghostpost.com/auth/callback`, `ghostpost://auth/callback` and
  `https://api.getghostpost.com/v1/webhooks/workos`.

The only thing that changes for clients is where `api.getghostpost.com`
resolves.

## Stack and ports

Compose project `ghostpost`, file `docker-compose.prod.yml`:

| Service | Role | Exposure |
|---|---|---|
| `postgres` | PostgreSQL 17, volume `postgres-data` | none |
| `migrate` | One-shot `migrate` with migrator credentials | none |
| `api` | `serve --role all` (HTTP API and worker), listens on `api:8080` | internal network only |
| `caddy` | TLS terminator, `docker/Caddyfile`, volumes `caddy-data` (`/data`) and `caddy-config` (`/config`) | TCP 80 and 443 |
| `backup` | Nightly `pg_dump`, `age`-encrypted, uploaded to R2 | none |

`cloudflared` and `CLOUDFLARE_TUNNEL_TOKEN` are gone. Only Caddy publishes host
ports, and only TCP 80 and 443; HTTP/3 (UDP) is disabled in the Caddyfile.
PostgreSQL 5432 and the API's 8080 must never be published.

Keep a host firewall that allows inbound TCP 22, 80 and 443 and drops the rest.
Docker publishes ports through its own netfilter rules, which bypass simple
`ufw allow` ordering, so the firewall is a second line, not the control that
keeps 5432 and 8080 closed. The control is the compose file: do not add
`ports:` to any service except `caddy`.

## Debian bootstrap

Run once on a fresh host, after the SSH key session below is verified. Use the
script; the manual list after it only explains what it does.

```bash
ssh "$GHOSTPOST_HOST" 'sudo bash -s -- <deploy-user>' < scripts/bootstrap-vps.sh
```

Argument 1 is the deploy user (the `<deploy-user>` placeholder above). It must
already exist, hold an authorized SSH key and have sudo rights, and will own
`/opt/ghostpost` and run the deploy scripts. The script is idempotent and safe
to re-run: it checks state first, never resets the firewall, never rewrites
`authorized_keys`, and only hardens
`sshd` after validating key access and `sshd -t` (it reloads `sshd`, it does
not restart it, so the open session survives). It does not print secrets.

What it does, for Debian bullseye, bookworm or trixie on amd64 or arm64 only:

1. Installs base packages (`ca-certificates`, `curl`, `gnupg`, `jq`, `rsync`,
   `ufw`, `unattended-upgrades`) and enables automatic security updates, with
   no automatic reboot.
2. Adds a 2 GiB swapfile only if the host has no swap and more than 12 GiB of
   disk is free.
3. Configures `ufw`: allow SSH (22 and the configured `sshd` ports), 80 and
   443, default deny incoming, IPv4 and IPv6. Rules are added before enabling,
   so the current session is never cut off.
4. Installs Docker Engine and the Compose plugin from Docker's official Debian
   apt repository, after checking the repository key fingerprint, with bounded
   `json-file` logging (20 MB, 5 files) merged into `daemon.json`. No
   convenience script is used.
5. Adds the deploy user to the `docker` group (log out and back in for it to
   apply).
6. Creates `/opt/ghostpost`, `releases` and `backups` owned by the deploy
   user with mode **0750** (not 755). The `chmod` is non-recursive, so existing
   releases and volumes are not re-owned.
7. Disables SSH password, keyboard-interactive and root login through
   `/etc/ssh/sshd_config.d/00-ghostpost-hardening.conf`, but only if the deploy
   user has an authorized key and full sudo; otherwise it warns and skips.

Afterwards, confirm from a new session that `docker compose version` and
`docker info` work for the deploy user without `sudo`, and that
`sudo ufw status` shows 22, 80 and 443 only. The firewall is a second line; see
[Stack and ports](#stack-and-ports).

### SSH key access (verified, private)

Never put a password in chat, a commit, a script or a shell history file shared
with others. Password authentication is used once, privately, to install a key.

1. Trust the host key deliberately. Compare the fingerprint that `ssh` shows on
   first connect with the one in the OVH panel or console. Do not accept it
   blindly.
2. Install the existing public key yourself, in your own terminal:
   `ssh-copy-id -i ~/.ssh/<key>.pub "$GHOSTPOST_HOST"`.
3. In a **new** terminal, verify key-only login actually works:
   `ssh -o PasswordAuthentication=no -o PreferredAuthentications=publickey "$GHOSTPOST_HOST" true`.
4. Only after step 3 succeeds, run the [bootstrap script](#debian-bootstrap)
   with the verified session still open. It disables SSH password
   authentication itself (only when key access and sudo are confirmed) and
   reloads `sshd`; re-run step 3 afterwards.

Every script below uses plain `ssh` with `GHOSTPOST_HOST`; it has no
default, so export it before running them.

## Scripts and interfaces

All scripts run from a laptop checkout. Inputs:

| Variable | Value |
|---|---|
| `GHOSTPOST_HOST` | required: SSH target, `user@host` |
| `GHOSTPOST_ENV_FILE` | `~/.config/ghostpost/production.env` (must already exist, mode 600) |
| Remote root | `/opt/ghostpost` |

Only keys listed in `.env.production.example` are shipped; all must be
non-empty. `CLOUDFLARE_TUNNEL_TOKEN` is no longer an allowed key. The encryption,
session, fingerprint and HMAC keys, and the Postgres role passwords, must be the
**same values** the previous production used (see
[Same keys](#same-keys-and-single-writer)).

Scripts deploy a selected immutable Git SHA (`git archive`), never the working
tree. Release directories live in `/opt/ghostpost/releases/<short-sha>`. The
current production release must be built from a commit that does not contain
the unrelated uncommitted onboarding, classifier or lint work in the working
tree; produce an infrastructure-only release commit and deploy that SHA.

### `bash scripts/deploy-vps.sh --prepare <sha>`

Stages a host without going live. It ships the release and secrets, builds the
backend and backup images, and starts PostgreSQL until healthy. That is all.

It does **not** run application migrations, start the API or worker, start
Caddy or the backup service, change DNS, or declare the release live.
PostgreSQL's one-time initialisation (roles and extensions from
`docker/postgres-prod-init.sh`) does happen, because it is part of starting a
fresh database.

### `bash scripts/restore-production.sh`

```bash
bash scripts/restore-production.sh \
  --release <sha> \
  --backup <local age-encrypted dump> \
  --restore-point <RFC3339> \
  [--identity <local age key>]
```

`--identity` defaults to `~/.config/ghostpost/backup-age.key`. Run it after
`--prepare` of the same release. Behaviour:

- Decrypts **locally** and streams the plaintext to the target over SSH. The age
  private key and the plaintext dump never reach the server's disk.
- Checks that the staged release directory matches the selected commit (full
  tree and env hashes), and refuses otherwise. `--release` is a Git SHA; the
  marker and release directory use its first 12 hex characters.
- Refuses to restore into a database that already holds application objects,
  or if the API, worker, Caddy or backup service is running.
- Restores with `pg_restore --single-transaction --exit-on-error` as the
  migrator role, so the restore commits entirely or not at all, then refreshes
  the `ghostpost_app` grants.
- Runs the selected release's migrations.
- Seeds the restore guard and runs the backend's existing `deletion replay`
  against the preserved R2 deletion ledger for markers newer than
  `--restore-point`, **before** any API or worker starts.
- Only after replay and sanity checks succeed does it atomically write
  `/opt/ghostpost/.database-restored`, JSON
  `{"release","restorePoint","restoredAt"}` (dates in UTC, `Z`) with no secrets.

There are no reset, drop or force-overwrite flags. Once the restore transaction
has committed, the script records `/opt/ghostpost/.restore-state` (release,
restore point, SHA-256 of the backup file and stage). If migration or replay
then fails, no marker is written and nothing is started. Fix the cause and
re-run the **identical** command (same release, same backup bytes, same
restore point): it resumes at migrate and replay without restoring again. Do
not delete the partially promoted database to retry. A different or replaced
backup, release or restore point, or a populated database without matching
state, is refused; that case needs a deliberate human decision about the
database, not a flag. A failed transfer that never committed leaves the
database empty and can simply be re-run. Do not hand-edit the marker or the
state file.

### `bash scripts/deploy-vps.sh <sha>`

Normal deployment. On a fresh target it **refuses** unless
`/opt/ghostpost/.database-restored` exists. Otherwise it builds, then:

1. takes `/opt/ghostpost/.deploy.lock`,
2. starts PostgreSQL and runs the `migrate` service with `--interactive=false`
   (without that, Compose can swallow the rest of the SSH-fed script),
3. starts `api`, waits for its internal `/health/ready`, then starts `caddy`
   and `backup`,
4. smokes the actual requested public API (`https://api.getghostpost.com`):
   it waits for `/health/ready`, then runs the health and WorkOS authorize
   smokes. The script does not silently skip public verification. The default
   production smoke must also verify that the public connection's peer IP is
   one of the target VPS's global IPs, so a healthy old API still answering at
   the name cannot pass for the new release. If the name does not resolve to
   the VPS yet, the smoke fails; you may override the URL
   (`GHOSTPOST_API_URL`) only for deliberate isolated verification, and the
   override is recorded in the deploy log,
5. only after step 4 passes, re-checks that the `api` container runs this
   release and is ready, then updates `/opt/ghostpost/current`, appends to
   `deploys.jsonl` and keeps the five newest releases.

If any check fails, `current` and `deploys.jsonl` are not updated. Services
already started stay running and nothing is rolled back automatically; fix the
cause and re-run (each step is idempotent), or run `--rollback` to a known
release.

A brand-new database with no restore (no users yet) is a separate decision from
a migration of existing data; do not create `.database-restored` by hand to get
around the refusal.

### `bash scripts/deploy-vps.sh --rollback <sha>`

Restarts a release that is already on the host and was previously deployed
successfully (listed in `deploys.jsonl` as a deploy or rollback), with its
images still present. It runs no migrations, never downgrades the database,
never overwrites secrets or data, and never switches back to the stale old host.

It refuses if the target release contains migrations that the database has not
applied. Beyond that, the script does **not** prove an older binary works on
the current schema: migrations are not guaranteed to be expand-only, and
compatibility of the older release with the newer schema and data is the
operator's responsibility. After the restart it goes through the same public
smoke and only then marks the release current.

A stale source database is never a rollback target once the VPS has accepted
writes (see [Rollback after new writes](#rollback-after-new-writes)).

## First cutover

Preferred order. Nothing below has been run for this move yet; treat each step
as a gate, not a log.

0. [Bootstrap](#debian-bootstrap) the host with `scripts/bootstrap-vps.sh`
   through the verified key session.
1. `--prepare <sha>`. Confirm Postgres is healthy and nothing else runs.
2. Obtain a data source, in order of preference:
   1. **Fresh final dump** from the live source database, taken after the old
      API and workers stop accepting writes. Encrypt it with the production
      `BACKUP_AGE_RECIPIENT` and keep the ciphertext locally.
   2. **Latest R2 backup** if the source is unreachable (see
      [Stale restore](#stale-restore)).
3. `restore-production.sh` with `--restore-point` set to a time **at or before**
   the start of the dump's snapshot: the recorded timestamp taken just before
   starting `pg_dump` for a final dump, or the timestamp in the backup
   filename for an R2 backup (the backup service takes it just before it starts
   the dump). Never use the time the dump finished: a deletion made while the
   dump was running may be absent from its snapshot, and a restore point after
   the snapshot start would make replay skip that ledger marker. An earlier
   restore point only replays more markers.
4. Add the DNS record ([DNS](#dns)) and wait for it to resolve.
5. `deploy-vps.sh <sha>`; Caddy obtains the certificate on first request.
6. Run the verification list in
   [the deployment plan](deployment-plan.md#verification-after-cutover).
7. **Stop the old backup scheduler** on the source, and any remaining service
   that writes to or dumps the old database. Two active backup writers produce
   two interleaved histories in the same R2 prefix, and a dump of the old
   database is stale by definition.

### Same keys and single writer

- Reuse the previous values of `ARCHIVE_FINGERPRINT_KEYS`,
  `SCAN_BATCH_HMAC_KEYS`, `APP_SESSION_KEYS`, `WORKOS_COOKIE_PASSWORD`, the
  database passwords and the backup age recipient. New values make restored
  fingerprints, in-flight scan batches and sessions fail verification, and a
  recipient change makes new backups undecryptable with the old key.
- At most one database may have an active API or worker at any moment. After
  cutover the old database must have none; do not start it again "just to
  look".

### Stale restore

The newest R2 backup found before this migration is
`postgres/2026/11/11/20261111T100246Z.dump.age` (264,453 bytes, 2026-11-11
10:02 UTC). Restoring it loses everything written after that time: accounts,
imports, scans and reviews. A stale restore needs explicit acceptance of that
data loss. Having the tool is not authorisation.

If you accept it:

- The 21-day ledger retention covers this backup's age, but only the replay
  stops deleted accounts from reappearing. It must succeed before any traffic.
- Deleted users' WorkOS state is independent of the database; replay is what
  re-applies the provider-side deletions the ledger records, so it must
  succeed before traffic.
- Users who signed up after the backup may have WorkOS identities with no
  database rows [INFERENCE]. Plan how they re-onboard.

### Rollback after new writes

Once the VPS has accepted any write, pointing `api.getghostpost.com` back at
the old database is **not** a rollback. It silently discards everything written
since the cutover, and it reopens a database that may be missing deletions
(the ledger replays only into the restored database). The safe options are to
fix forward, or to take a dump from the VPS and restore that into whichever
host should be active, with the other stopped. A DNS revert is acceptable only
before the first write, and only if the old database was untouched.

## DNS

- Add `api.getghostpost.com` as an API-managed **A** record to
  `the VPS public IPv4 address`, DNS-only (not proxied) so Caddy answers directly and ACME
  validation reaches the VPS.
- Remove the old tunnel `CNAME` for the name at cutover, not before. Public
  health was observed returning HTTP 530 and the source VM does not answer
  from here; the cause of the 530 has not been established.
- Add an **AAAA** record for the VPS public IPv6 address only after you have
  verified from outside the VPS that IPv6 reaches Caddy on 80 and 443 and the
  certificate is served. An AAAA that does not answer breaks clients that
  prefer IPv6.
- Leave all `app.` and `getghostpost.com` records alone.

## Certificates

Caddy gets and renews the `api.getghostpost.com` certificate through ACME. It
needs:

- the A record resolving to this VPS before the first request,
- inbound TCP 80 and 443 reachable from the internet,
- the `caddy-data` volume (`/data`) persisted. It holds the account and
  certificates; do not delete it during redeploys (`docker compose down -v` is
  forbidden here). Recreating it reissues and can hit CA rate limits,
- outbound access to the CA.

`/config` (`caddy-config`) holds Caddy's autosaved configuration.

## Storage layout

- `/opt/ghostpost/releases/<sha>`: unpacked releases (5 kept).
- `/opt/ghostpost/current`: symlink to the active release.
- `/opt/ghostpost/.env.production`: mode 600, shipped by the deploy scripts.
- `/opt/ghostpost/.database-restored`: written only by the restore tool.
- `/opt/ghostpost/.deploy.lock`, `deploys.jsonl`.
- Named volumes:
  - `ghostpost_postgres-data`: PostgreSQL data directory.
  - `ghostpost_import-spool`: `/var/spool/ghostpost`, disk-backed (never tmpfs).
  - `ghostpost_backup-spool`: `/var/spool/ghostpost-backup`, ciphertext only.
  - `ghostpost_caddy-data`, `ghostpost_caddy-config`.

## Capacity

- **Import spool.** The importer no longer holds the archive in memory. It
  spools the pinned archive to an unlinked temporary file in
  `IMPORT_SPOOL_DIR` (`/var/spool/ghostpost`) and streams entries through
  bounded parsers (`backend/src/jobs/archive_import.rs`). Peak memory does not
  grow with archive size, but disk does: budget up to `MAX_ARCHIVE_BYTES`
  (2 GiB) per concurrent import on the Docker volume's filesystem, plus the
  imported rows in PostgreSQL.
- **PostgreSQL.** The container sets `shm_size: 256mb`. Size disk for the data
  directory, WAL and one extra copy of a dump during restore. Do not guess
  memory settings; measure after real imports.
- **Native Rust build.** The backend image compiles on the VPS (`dc build`),
  which needs CPU time, RAM for the linker and disk for build cache and image
  layers. There is no benchmark for this host. Run the first `--prepare`, watch
  `docker stats`, `free -h` and `df -h`, and record the result. If the build is
  killed or the disk fills, resize the VPS or prune the build cache; do not
  change the release to hide it.

## Backups

The `backup` service writes `postgres/YYYY/MM/DD/<timestamp>.dump.age` nightly
to R2 `ghostpost-backups` (lifecycle: 14 days). The private age key stays on
the laptop at `~/.config/ghostpost/backup-age.key`; it is never copied to the
VPS. A restore rehearsal has **not** been run for this host. Run one into a
throwaway target before relying on backups here.

## Appendix: legacy HTPC source recovery

Retained only to read data from the previous production VM if it comes back.
It is not a production bootstrap, and `scripts/htpc/` must not be used to
create the new host. Facts below describe the old system and are unverified
today: the laptop's `ghostpost-vm` ProxyJump through `htpc` currently fails
with "No route to host", and the cause has not been diagnosed at the
hypervisor.

The old VM ran inside libvirt on `htpc-ubuntu` with LAN egress blocked at the
hypervisor level (ebtables nwfilter).

- Domain: `ghostpost` (autostarts on host boot)
- Specs: 4 vCPU, 6 GiB RAM, 40 GB qcow2 (backing: Noble cloudimg)
- Network: `ghostpost` (192.168.150.0/24 NAT, bridge `virbr-ghost`)
- Static IP: `192.168.150.10` (MAC `52:54:00:67:70:01`)
- Laptop SSH alias: `ghostpost-vm` (ProxyJump through `htpc`)
- Provisioning files, kept for recovery only: `scripts/htpc/create-vm.sh`
  (idempotent; downloads and checks the Ubuntu 24.04 cloud image, defines the
  `ghostpost-no-lan` and `ghostpost-egress` nwfilters, the `ghostpost` network
  and pool, then `virt-install` with cloud-init), `network.xml`,
  `nwfilter-*.xml`, `user-data.yaml`.

Egress isolation: the VM interface used `filterref.filter=ghostpost-egress`,
which evaluated `ghostpost-no-lan` first. It accepted DNS (UDP/TCP 53) to
`192.168.150.1` and replies on TCP source port 22 for the host's ProxyJump, and
dropped other traffic to `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`,
`169.254.0.0/16`, `100.64.0.0/10` and all IPv6.

Old layout inside the VM: `/opt/ghostpost/releases/<sha>`,
`/opt/ghostpost/current`, `/opt/ghostpost/.env.production` and named volumes
`ghostpost_postgres-data`, `ghostpost_import-spool`, `ghostpost_backup-spool`.
It ran `cloudflared` for ingress.

Recovery order when the source is reachable again: stop its API and worker
first, take a fresh dump of the database, encrypt it with the backup recipient,
and use [the first cutover](#first-cutover). When it stays unreachable, the R2
backup path applies, with the stale-restore conditions above. The old backup
scheduler (the old `backup` service) must stay stopped once the VPS is live.
