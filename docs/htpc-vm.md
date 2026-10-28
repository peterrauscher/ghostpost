# `ghostpost` VM runbook (HTPC)

The production VM runs inside libvirt on `htpc-ubuntu` with LAN egress blocked
at the hypervisor level (ebtables nwfilter).

- Domain: `ghostpost` (autostarts on host boot)
- Specs: 4 vCPU, 6 GiB RAM, 40 GB qcow2 (backing: Noble cloudimg)
- Network: `ghostpost` (192.168.150.0/24 NAT, bridge `virbr-ghost`)
- Static IP: `192.168.150.10` (MAC `52:54:00:67:70:01`)
- Laptop SSH alias: `ghostpost-vm` (ProxyJump through `htpc`)

## Recreating the VM from scratch

```bash
# From this laptop (needs ~/.ssh/id_ed25519.pub):
bash scripts/htpc/create-vm.sh
```

The script is idempotent: it downloads and SHA-256 checks the official
Ubuntu 24.04 cloud image, defines the `ghostpost-no-lan` and `ghostpost-egress`
nwfilters, defines and starts the `ghostpost` network and storage pool,
creates the backed root disk, and runs `virt-install` with cloud-init.

## Egress isolation

The VM interface is attached to `filterref.filter=ghostpost-egress`, which
evaluates `ghostpost-no-lan` before allowing any frame:

- Accepts UDP and TCP port 53 to `192.168.150.1` (DNS resolution through the host's libvirt dnsmasq).
- Accepts TCP source port 22 to `192.168.150.1` (replies to the host's ProxyJump SSH session into the VM).
- Drops all other traffic to `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `169.254.0.0/16`, `100.64.0.0/10` (Tailscale CGNAT), and all IPv6.

The VM cannot reach any host service (Plex, Immich, Docker bridges) or the
home LAN (`192.168.4.0/22`), but can reach the public internet (Cloudflare,
R2, WorkOS, Typesafe). Verified by `scripts/htpc/create-vm.sh` and regular
smoke tests.

## Storage layout on the VM

- `/opt/ghostpost/releases/<sha>`: unpacked release directories (5 kept).
- `/opt/ghostpost/current`: symlink to the active release.
- `/opt/ghostpost/.env.production`: mode 600, shipped by `scripts/deploy-htpc.sh`.
- Named volumes:
  - `ghostpost_postgres-data`: Postgres 17 data directory.
  - `ghostpost_import-spool`: mounted at `/var/spool/ghostpost`, disk-backed (never tmpfs).
  - `ghostpost_backup-spool`: mounted at `/var/spool/ghostpost-backup`, ciphertext only.
