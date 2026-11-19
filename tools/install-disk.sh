#!/usr/bin/env bash
# Install OCTAGRAM-OS to an internal SATA disk (UEFI/GPT + EXT2 root).
# Usage: sudo ./tools/install-disk.sh /dev/sdX [path/to/kernel]
set -Eeuo pipefail

DISK="${1:-}"
KERNEL="${2:-kernel/kernel}"
ROOT_LABEL="OCTAGRAM_ROOT"
EFI_LABEL="OCTAGRAM_EFI"
WORK="$(mktemp -d)"
cleanup() { mountpoint -q "$WORK/efi" && umount "$WORK/efi" || true; mountpoint -q "$WORK/root" && umount "$WORK/root" || true; rm -rf "$WORK"; }
trap cleanup EXIT

die() { echo "ERROR: $*" >&2; exit 1; }
[[ $EUID -eq 0 ]] || die "Run with sudo."
[[ -n "$DISK" ]] || die "Usage: sudo $0 /dev/sdX [kernel-file]"
[[ -b "$DISK" ]] || die "$DISK is not a block device."
[[ -f "$KERNEL" ]] || die "Kernel not found: $KERNEL"
[[ "$(lsblk -dn -o TYPE "$DISK")" == "disk" ]] || die "Pass a whole disk, not a partition."
for cmd in lsblk sgdisk partprobe mkfs.vfat mkfs.ext2 mount umount blkid; do
  command -v "$cmd" >/dev/null || die "Missing required command: $cmd"
done

echo "TARGET DISK:"
lsblk -o NAME,SIZE,MODEL,TYPE,MOUNTPOINTS "$DISK"
echo
echo "WARNING: ALL DATA ON $DISK WILL BE DESTROYED."
read -r -p "Type exactly ERASE-$DISK to continue: " answer
[[ "$answer" == "ERASE-$DISK" ]] || die "Confirmation did not match."

# Unmount any mounted child partitions before replacing the partition table.
while read -r node; do
  [[ -n "$node" ]] || continue
  if findmnt -rn -S "$node" >/dev/null; then umount "$node"; fi
done < <(lsblk -lnpo NAME "$DISK" | tail -n +2)

sgdisk --zap-all "$DISK"
sgdisk --clear \
  --new=1:2048:+512M --typecode=1:EF00 --change-name=1:"$EFI_LABEL" \
  --new=2:0:0 --typecode=2:8300 --change-name=2:"$ROOT_LABEL" "$DISK"
partprobe "$DISK"
udevadm settle || true

if [[ "$DISK" =~ [0-9]$ ]]; then
  EFI_PART="${DISK}p1"; ROOT_PART="${DISK}p2"
else
  EFI_PART="${DISK}1"; ROOT_PART="${DISK}2"
fi
for _ in {1..30}; do [[ -b "$EFI_PART" && -b "$ROOT_PART" ]] && break; sleep 1; done
[[ -b "$EFI_PART" && -b "$ROOT_PART" ]] || die "Partition nodes did not appear."

mkfs.vfat -F 32 -n "$EFI_LABEL" "$EFI_PART"
mkfs.ext2 -F -L "$ROOT_LABEL" "$ROOT_PART"
mkdir -p "$WORK/efi" "$WORK/root"
mount "$EFI_PART" "$WORK/efi"
mount "$ROOT_PART" "$WORK/root"

mkdir -p "$WORK/efi/EFI/BOOT" "$WORK/efi/boot/limine" \
  "$WORK/root"/{dev,bin,lib,lib/tcc,share/octagram,include,include/sys,tmp,home/octagram,etc,var/log,var/lib/octagram/packages,opt,apps,usr/bin,usr/local/bin,mnt}
cp "$KERNEL" "$WORK/efi/boot/kernel"
cp limine.conf "$WORK/efi/boot/limine/limine.conf"

# Limine's removable-media UEFI loader makes the disk independently bootable.
[[ -f limine/BOOTX64.EFI ]] || die "Build Limine first (make limine/limine)."
cp limine/BOOTX64.EFI "$WORK/efi/EFI/BOOT/BOOTX64.EFI"
[[ -f limine/BOOTIA32.EFI ]] && cp limine/BOOTIA32.EFI "$WORK/efi/EFI/BOOT/BOOTIA32.EFI" || true
[[ -f limine/limine-bios.sys ]] && cp limine/limine-bios.sys "$WORK/efi/boot/limine/" || true

printf '%s\n' 'OCTAGRAM-OS persistent root v1' > "$WORK/root/etc/octagram-root"
printf '%s\n' 'OCTAGRAM_OS_NAME="OCTAGRAM-OS"' 'OCTAGRAM_OS_ID=octagram' 'OCTAGRAM_OS_VERSION=1.0' > "$WORK/root/etc/os-release"
printf '%s\n' 'octagram:x:0:0:OCTAGRAM User:/home/octagram:/bin/sh' > "$WORK/root/etc/passwd"
printf '%s\n' 'export PATH=/bin:/usr/bin:/usr/local/bin' > "$WORK/root/home/octagram/.profile"
printf '%s\n' '# OCTAGRAM native application registry' > "$WORK/root/apps/apps.txt"
: > "$WORK/root/var/lib/octagram/packages/registry.txt"
sync
umount "$WORK/root"
umount "$WORK/efi"

echo
echo "OCTAGRAM-OS installed."
echo "EFI system partition: $EFI_PART"
echo "EXT2 persistent root: $ROOT_PART"
echo "Select the disk's UEFI entry in firmware boot menu."
echo "Note: current kernel auto-detects SATA/AHCI disks only; NVMe is not supported yet."
