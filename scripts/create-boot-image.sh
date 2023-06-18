#!/usr/bin/env bash

# Used by the Makefile to create a bootable disk image with the kernel and limine
#
# Adapted from https://github.com/limine-bootloader/limine-barebones/blob/trunk/GNUmakefile

set -eu

if [ $# -ne 3 ]; then
    echo "Usage: $0 <kernel_hdd> <kernel_binary> <cmdline>"
    exit 1
fi

cd "$(dirname "$0")/.."

kernel_hdd=$1
kernel_binary=$2
cmdline=$3

limine_dir=$(nix build ./flake#limine --print-out-paths --no-link)

echo "kernel_hdd: $kernel_hdd"
echo "kernel_binary: $kernel_binary"
echo "limine_dir: $limine_dir"
echo "cmdline: $cmdline"

rm -f "$kernel_hdd"
dd if=/dev/zero bs=1M count=0 seek=64 of="$kernel_hdd"
parted -s "$kernel_hdd" mklabel gpt
parted -s "$kernel_hdd" mkpart ESP fat32 2048s 100%
parted -s "$kernel_hdd" set 1 esp on
"$limine_dir/limine-deploy" "$kernel_hdd"

loopback_dev=$(sudo losetup -Pf --show "$kernel_hdd")
echo "loopback_dev: $loopback_dev"
sudo mkfs.fat -F 32 "${loopback_dev}p1"
mkdir -p img_mount
sudo mount "${loopback_dev}p1" img_mount
sudo mkdir -p img_mount/EFI/BOOT
sudo cp -v "$kernel_binary" img_mount/kernel.elf

# Run nm to create a map of all the kernel's symbols. Useful for stack traces
sudo nm "$kernel_binary" | sudo tee img_mount/kernel.symbols > /dev/null

sudo cp -v limine.cfg "$limine_dir/limine.sys" img_mount/
sudo sed -i "s|CMDLINE=|CMDLINE=$cmdline|" img_mount/limine.cfg
sudo cp -v "$limine_dir/BOOTX64.EFI" img_mount/EFI/BOOT/
sync img_mount

sudo umount img_mount
sudo losetup -d "$loopback_dev"
rm -rf img_mount
