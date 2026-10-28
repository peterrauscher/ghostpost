#!/usr/bin/env bash
# Creates the isolated `ghostpost` VM on htpc. Run from the laptop:
#   bash scripts/htpc/create-vm.sh
# Needs only the htpc user's libvirt group membership (no host sudo).
# Idempotent for the network/filter/pool; refuses to touch an existing domain.
set -euo pipefail

HOST="${HTPC_HOST:-htpc}"
PUBKEY_FILE="${PUBKEY_FILE:-$HOME/.ssh/id_ed25519.pub}"
HERE="$(cd "$(dirname "$0")" && pwd)"
[[ -f "$PUBKEY_FILE" ]] || { echo "missing $PUBKEY_FILE" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
sed "s|__SSH_PUBKEY__|$(cat "$PUBKEY_FILE")|" "$HERE/user-data.yaml" >"$WORK/user-data"
printf 'instance-id: ghostpost-1\nlocal-hostname: ghostpost\n' >"$WORK/meta-data"
scp -q "$WORK/user-data" "$WORK/meta-data" "$HERE/network.xml" "$HERE/nwfilter-egress.xml" "$HERE/nwfilter-no-lan.xml" "$HOST:/tmp/"

ssh "$HOST" 'bash -s' <<'REMOTE'
set -euo pipefail
V="virsh -c qemu:///system"
IMG_URL=https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img
SUMS_URL=https://cloud-images.ubuntu.com/noble/current/SHA256SUMS
CACHE="$HOME/.cache/ghostpost-vm"; mkdir -p "$CACHE"

if $V dominfo ghostpost >/dev/null 2>&1; then echo "domain ghostpost already exists; refusing" >&2; exit 1; fi

echo "==> cloud image"
curl -fsSL "$SUMS_URL" -o "$CACHE/SHA256SUMS"
curl -fsSL -z "$CACHE/noble.img" "$IMG_URL" -o "$CACHE/noble.img"
want="$(awk '$2=="*noble-server-cloudimg-amd64.img"{print $1}' "$CACHE/SHA256SUMS")"
got="$(sha256sum "$CACHE/noble.img" | cut -d' ' -f1)"
[[ -n "$want" && "$want" == "$got" ]] || { echo "cloud image checksum mismatch" >&2; exit 1; }

echo "==> egress filters"
define_filter() { # redefining an existing nwfilter requires its UUID
  local name="$1" file="$2" uuid
  if uuid="$($V nwfilter-dumpxml "$name" 2>/dev/null | grep -o '<uuid>[^<]*</uuid>')"; then
    sed -i "s|^<filter name='$name'[^>]*>|&\n  $uuid|" "$file"
  fi
  $V nwfilter-define "$file" >/dev/null
}
define_filter ghostpost-no-lan /tmp/nwfilter-no-lan.xml
define_filter ghostpost-egress /tmp/nwfilter-egress.xml

echo "==> network"
if ! $V net-info ghostpost >/dev/null 2>&1; then $V net-define /tmp/network.xml >/dev/null; fi
$V net-autostart ghostpost >/dev/null
$V net-info ghostpost | grep -q 'Active:.*yes' || $V net-start ghostpost >/dev/null

echo "==> storage pool"
if ! $V pool-info ghostpost >/dev/null 2>&1; then
  $V pool-define-as ghostpost dir --target /var/lib/libvirt/images/ghostpost >/dev/null
  $V pool-build ghostpost >/dev/null
fi
$V pool-autostart ghostpost >/dev/null
$V pool-info ghostpost | grep -q 'State:.*running' || $V pool-start ghostpost >/dev/null
if ! $V vol-info --pool ghostpost noble-base.img >/dev/null 2>&1; then
  $V vol-create-as ghostpost noble-base.img "$(stat -c %s "$CACHE/noble.img")" --format qcow2 >/dev/null
  $V vol-upload --pool ghostpost noble-base.img "$CACHE/noble.img"
fi
BASE="$($V vol-path --pool ghostpost noble-base.img)"
$V vol-create-as ghostpost ghostpost-root.qcow2 40G --format qcow2 --backing-vol "$BASE" --backing-vol-format qcow2 >/dev/null

echo "==> domain"
virt-install --connect qemu:///system --name ghostpost \
  --memory 6144 --vcpus 4 --cpu host-passthrough \
  --osinfo ubuntu24.04 \
  --disk vol=ghostpost/ghostpost-root.qcow2,bus=virtio \
  --network network=ghostpost,model=virtio,mac=52:54:00:67:70:01,filterref.filter=ghostpost-egress \
  --cloud-init user-data=/tmp/user-data,meta-data=/tmp/meta-data \
  --graphics none --console pty,target_type=serial \
  --autostart --noautoconsole --import
rm -f /tmp/user-data /tmp/meta-data /tmp/network.xml /tmp/nwfilter-*.xml
echo "==> created; cloud-init continues inside the VM"
REMOTE
