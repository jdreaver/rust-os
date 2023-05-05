#!/usr/bin/env bash

set -eu

rm -rf test-image
mkdir -p test-image

name="test-image/fat12.img"
block_count=2000

dd if=/dev/zero of="$name" bs=1024 "count=$block_count"
mformat -i "$name"

for i in $(seq 1 1000); do
  echo "Rust is cool!" >> "test-image/long.txt"
done
mcopy -i "$name" "test-image/long.txt" ::
echo "Rust is cool!" >> "test-image/short.txt"
mcopy -i "$name" "test-image/short.txt" ::
