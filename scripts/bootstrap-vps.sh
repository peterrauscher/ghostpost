#!/usr/bin/env bash
# Idempotent provisioning of the Ghostpost production VPS (Debian).
#
# Run from the operator machine through an ALREADY-VERIFIED SSH key session:
#
#   ssh "$GHOSTPOST_HOST" 'sudo bash -s' < scripts/bootstrap-vps.sh
#
# Optional argument 1 is the deploy user that will own /opt/ghostpost and run
# scripts/deploy-vps.sh (default: the sudo-invoking user). Must run as root
# (via sudo); the deploy user must already exist, hold >=1 authorized SSH key
# and have sudo rights, otherwise SSH password/root hardening is skipped.
#
# Safe to re-run: every step checks state first. It never resets the firewall,
# never rewrites authorized_keys, and only hardens sshd after the key check and
# `sshd -t` validation (the existing session survives; sshd is reloaded, not
# restarted). Does not print secrets.
#
# Firewall note: Docker publishes container ports through its own iptables
# chains, bypassing ufw INPUT rules. Safety therefore rests on the compose file
# publishing only Caddy's TCP 80/443; Postgres/API are never published.
#
# The whole script lives in main() so that bash, reading it from stdin, parses
# it completely before any child process can consume the stream.

set -Eeuo pipefail

main() {
  exec </dev/null
  export DEBIAN_FRONTEND=noninteractive
  export LC_ALL=C

  log() { printf '[bootstrap] %s\n' "$*"; }
  warn() { printf '[bootstrap] WARNING: %s\n' "$*" >&2; }
  die() { printf '[bootstrap] ERROR: %s\n' "$*" >&2; exit 1; }
  trap 'printf "[bootstrap] ERROR: failed at line %s\n" "$LINENO" >&2' ERR

  readonly APT=(apt-get -o DPkg::Lock::Timeout=300 -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold -y)
  readonly APP=/opt/ghostpost
  readonly DOCKER_KEY_FPR=9DC858229FC7DD38854AE2D88D81803C0EBFCD88

  [ "$(id -u)" -eq 0 ] || die "must run as root (use: sudo bash -s -- <deploy-user>)"

  local deploy_user="${1:-${SUDO_USER:-}}"
  [ -n "$deploy_user" ] || die "deploy user required: bash -s -- <deploy-user>"
  [ "$deploy_user" != root ] || die "deploy user must not be root"
  id -u "$deploy_user" >/dev/null 2>&1 || die "deploy user '$deploy_user' does not exist"
  local deploy_group deploy_home
  deploy_group="$(id -gn "$deploy_user")"
  deploy_home="$(getent passwd "$deploy_user" | cut -d: -f6)"
  [ -d "$deploy_home" ] || die "home directory of '$deploy_user' not found"

  # --- Platform detection (reject anything but supported Debian) -------------
  [ -r /etc/os-release ] || die "/etc/os-release missing; unsupported system"
  local os_id codename arch
  # /etc/os-release exists only on the Debian target, not on the validating workstation.
  # shellcheck source=/dev/null
  os_id="$(. /etc/os-release && printf '%s' "${ID:-}")"
  # shellcheck source=/dev/null
  codename="$(. /etc/os-release && printf '%s' "${VERSION_CODENAME:-}")"
  [ "$os_id" = debian ] || die "unsupported distro '$os_id' (Debian only)"
  case "$codename" in
    bullseye | bookworm | trixie) ;;
    *) die "unsupported Debian release '$codename' (need bullseye, bookworm or trixie)" ;;
  esac
  arch="$(dpkg --print-architecture)"
  case "$arch" in
    amd64 | arm64) ;;
    *) die "unsupported architecture '$arch' (need amd64 or arm64)" ;;
  esac
  command -v systemctl >/dev/null || die "systemd required"
  command -v flock >/dev/null || die "flock (util-linux) required"
  log "debian $codename/$arch, deploy user $deploy_user"

  install -d -m 0755 /run/lock
  exec 9>/run/lock/ghostpost-bootstrap.lock
  flock -n 9 || die "another bootstrap is already running"

  # write_if_changed <dest> <mode>: stdin -> dest atomically. Success (status 0)
  # covers both "written" and "already identical"; WRITE_CHANGED is set to 1 or
  # 0 accordingly. Any real I/O failure aborts the whole bootstrap via die,
  # independent of the caller's `||`/`&&` context (which suppresses errexit).
  # Must be called in the main shell (never as a pipeline stage: a subshell
  # would swallow die's exit); feed stdin with a heredoc or redirect.
  WRITE_CHANGED=0
  write_if_changed() {
    local dest="$1" mode="$2" tmp
    WRITE_CHANGED=0
    tmp="$(mktemp "${dest}.XXXXXX")" || die "cannot create temp file beside $dest"
    cat >"$tmp" || { rm -f "$tmp"; die "cannot write $dest"; }
    chmod "$mode" "$tmp" || { rm -f "$tmp"; die "cannot chmod temp file for $dest"; }
    if [ -f "$dest" ] && cmp -s "$tmp" "$dest"; then
      rm -f "$tmp" || die "cannot remove temp file for $dest"
      return 0
    fi
    mv -f "$tmp" "$dest" || { rm -f "$tmp"; die "cannot replace $dest"; }
    WRITE_CHANGED=1
  }

  # --- Base packages ---------------------------------------------------------
  log "installing base packages"
  "${APT[@]}" update
  "${APT[@]}" install ca-certificates curl gnupg iproute2 jq rsync tar ufw unattended-upgrades util-linux
  for tool in ip jq flock tar curl rsync; do
    command -v "$tool" >/dev/null || die "required tool '$tool' missing after package install"
  done

  # --- Automatic security updates (no automatic reboot) ----------------------
  write_if_changed /etc/apt/apt.conf.d/20auto-upgrades 0644 <<'EOF'
APT::Periodic::Update-Package-Lists "1";
APT::Periodic::Unattended-Upgrade "1";
APT::Periodic::AutocleanInterval "7";
EOF
  systemctl enable --now unattended-upgrades.service

  # --- Swap (only when the host has none and disk is ample) ------------------
  if [ -z "$(swapon --noheadings --show 2>/dev/null)" ] && [ ! -e /swapfile ]; then
    local avail_kb
    avail_kb="$(df --output=avail -k / | tail -n1 | tr -d ' ')"
    if [ "$avail_kb" -gt $((12 * 1024 * 1024)) ]; then
      log "creating 2G swapfile"
      fallocate -l 2G /swapfile 2>/dev/null || dd if=/dev/zero of=/swapfile bs=1M count=2048 status=none
      chmod 600 /swapfile
      mkswap /swapfile >/dev/null
      swapon /swapfile
      grep -qs '^/swapfile ' /etc/fstab || printf '/swapfile none swap sw 0 0\n' >>/etc/fstab
    else
      warn "skipping swap: less than 12GiB free"
    fi
  fi

  # --- Firewall: SSH + Caddy only, IPv4 and IPv6, never reset ----------------
  log "configuring ufw"
  local ssh_ports ssh_port ufw_was_active=0 ipv6_changed=0
  ssh_ports="$( (sshd -T 2>/dev/null || true) | awk '$1 == "port" {print $2}' | sort -un)"
  [ -n "$ssh_ports" ] || ssh_ports=22
  if ufw status 2>/dev/null | grep -q '^Status: active'; then ufw_was_active=1; fi

  if [ -f /etc/default/ufw ]; then
    if grep -q '^IPV6=' /etc/default/ufw; then
      if ! grep -q '^IPV6=yes' /etc/default/ufw; then
        sed -i 's/^IPV6=.*/IPV6=yes/' /etc/default/ufw
        ipv6_changed=1
      fi
    else
      printf 'IPV6=yes\n' >>/etc/default/ufw
      ipv6_changed=1
    fi
  fi

  # Rules first, enable last: the current SSH session can never be cut off.
  ufw allow 22/tcp >/dev/null
  for ssh_port in $ssh_ports; do
    ufw allow "${ssh_port}/tcp" >/dev/null
  done
  ufw allow 80/tcp >/dev/null
  ufw allow 443/tcp >/dev/null
  if [ "$ufw_was_active" -eq 0 ]; then
    ufw default deny incoming >/dev/null
    ufw default allow outgoing >/dev/null
    ufw --force enable >/dev/null
  elif [ "$ipv6_changed" -eq 1 ]; then
    ufw reload >/dev/null
  fi
  systemctl enable ufw.service >/dev/null 2>&1 || true

  # --- Docker daemon config (bounded logs), merged with any existing one -----
  log "configuring docker daemon logging"
  install -d -m 0755 /etc/docker
  local daemon_json=/etc/docker/daemon.json docker_config_changed=0 tmp
  if [ -s "$daemon_json" ]; then
    jq -e . "$daemon_json" >/dev/null || die "$daemon_json is not valid JSON; refusing to overwrite"
    if ! jq -e '."log-driver" == "json-file" and ."log-opts"."max-size" == "20m" and ."log-opts"."max-file" == "5"' "$daemon_json" >/dev/null; then
      tmp="$(mktemp "${daemon_json}.XXXXXX")"
      jq '. + {"log-driver": "json-file"} | ."log-opts" = ((."log-opts" // {}) + {"max-size": "20m", "max-file": "5"})' "$daemon_json" >"$tmp"
      chmod 0644 "$tmp"
      mv -f "$tmp" "$daemon_json"
      docker_config_changed=1
    fi
  else
    write_if_changed "$daemon_json" 0644 <<'EOF'
{
  "log-driver": "json-file",
  "log-opts": { "max-size": "20m", "max-file": "5" }
}
EOF
    docker_config_changed="$WRITE_CHANGED"
  fi

  # --- Docker apt repository for Debian --------------------------------------
  log "configuring docker apt repository"
  install -d -m 0755 /etc/apt/keyrings
  local key_tmp
  key_tmp="$(mktemp)"
  curl -fsSL https://download.docker.com/linux/debian/gpg -o "$key_tmp"
  gpg --show-keys --with-colons "$key_tmp" | grep -q "^fpr:::::::::${DOCKER_KEY_FPR}:" \
    || { rm -f "$key_tmp"; die "Docker apt key fingerprint mismatch"; }
  write_if_changed /etc/apt/keyrings/docker.asc 0644 <"$key_tmp"
  rm -f "$key_tmp"
  write_if_changed /etc/apt/sources.list.d/docker.sources 0644 <<EOF
Types: deb
URIs: https://download.docker.com/linux/debian
Suites: $codename
Components: stable
Architectures: $arch
Signed-By: /etc/apt/keyrings/docker.asc
EOF
  "${APT[@]}" update
  "${APT[@]}" install docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  systemctl enable --now containerd.service docker.service
  if [ "$docker_config_changed" -eq 1 ] && systemctl is-active --quiet docker.service; then
    # Bounded-log options only take effect after a daemon restart.
    systemctl restart docker.service
  fi
  docker compose version >/dev/null || die "docker compose plugin not working"

  if ! id -nG "$deploy_user" | tr ' ' '\n' | grep -qx docker; then
    usermod -aG docker "$deploy_user"
  fi

  # --- Application directories ----------------------------------------------
  # Non-recursive: existing release contents / volumes are never re-owned.
  install -d -o "$deploy_user" -g "$deploy_group" -m 0750 "$APP" "$APP/releases" "$APP/backups"
  chown "$deploy_user:$deploy_group" "$APP" "$APP/releases" "$APP/backups"
  chmod 0750 "$APP" "$APP/releases" "$APP/backups"

  # --- SSH hardening: only once key-based access is proven present ----------
  harden_ssh() {
    local auth_keys="$deploy_home/.ssh/authorized_keys"
    if [ ! -s "$auth_keys" ] || ! grep -Eqs '^[[:space:]]*(ssh-|ecdsa-|sk-)' "$auth_keys"; then
      warn "no authorized key for $deploy_user: SSH password/root hardening SKIPPED"
      return 0
    fi
    if ! sudo -l -U "$deploy_user" 2>/dev/null | grep -Eq '\(ALL( ?: ?ALL)?\) +(NOPASSWD: +)?ALL'; then
      warn "$deploy_user has no full sudo rights: SSH password/root hardening SKIPPED"
      return 0
    fi
    if ! grep -Eqi '^[[:space:]]*Include[[:space:]]+/etc/ssh/sshd_config\.d/' /etc/ssh/sshd_config; then
      warn "sshd_config has no sshd_config.d include: SSH hardening SKIPPED"
      return 0
    fi
    install -d -m 0755 /run/sshd /etc/ssh/sshd_config.d
    local dropin=/etc/ssh/sshd_config.d/00-ghostpost-hardening.conf backup=""
    if [ -f "$dropin" ]; then
      backup="$(mktemp)"
      cp -p "$dropin" "$backup"
    fi
    write_if_changed "$dropin" 0644 <<'EOF'
# Managed by scripts/bootstrap-vps.sh. Sorts first so it wins over cloud images' drop-ins.
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin no
EOF
    local effective
    if sshd -t && effective="$(sshd -T)" \
      && grep -qx 'passwordauthentication no' <<<"$effective" \
      && grep -qx 'kbdinteractiveauthentication no' <<<"$effective" \
      && grep -qx 'permitrootlogin no' <<<"$effective"; then
      systemctl reload ssh.service 2>/dev/null || systemctl reload sshd.service
      [ -z "$backup" ] || rm -f "$backup"
      log "ssh hardened (password and root login disabled)"
    else
      if [ -n "$backup" ]; then mv -f "$backup" "$dropin"; else rm -f "$dropin"; fi
      die "sshd validation failed; hardening reverted, SSH access untouched"
    fi
  }
  harden_ssh

  # --- Final state -----------------------------------------------------------
  date -u +%FT%TZ >/var/lib/ghostpost-provisioned
  log "docker: $(docker --version)"
  log "ufw: $(ufw status | head -n1)"
  log "done"
}

main "$@"
exit
