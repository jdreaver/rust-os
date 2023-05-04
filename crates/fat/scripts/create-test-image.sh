#!/usr/bin/env bash

set -eu

name="fat12.img"
block_count=2000

dd if=/dev/zero of="$name" bs=1024 "count=$block_count"
mkfs.vfat -s 1 -F 12 -n "Test!" -i 12345678 "$name"
mkdir -p mnt
sudo mount -o loop "$name" mnt -o rw,uid="$USER",gid="$USER"

for i in $(seq 1 1000); do
  echo "Rust is cool!" >>"mnt/long.txt"
done

echo "Rust is cool!" >>"mnt/short.txt"
mkdir -p "mnt/very/long/path"
echo "Rust is cool!" >>"mnt/very/long/path/test.txt"
mkdir -p "mnt/very-long-dir-name"
echo "Rust is cool!" >>"mnt/very-long-dir-name/very-long-file-name.txt"

sudo umount mnt
