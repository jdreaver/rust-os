#!/usr/bin/env bash

set -eu

if [ "$#" -ne 1 ]; then
  echo "Usage: $0 OUTPUT_FILE" >&2
  exit 1
fi

output_file="$1"

dd if=/dev/zero of="$output_file" bs=1024 count=2000
mformat -i "$output_file"

long_txt=/tmp/long.txt
rm -f "$long_txt"
for i in $(seq 1 1000); do
  echo "Rust is cool!" >> "$long_txt"
done
mcopy -i "$output_file" "$long_txt" ::

short_txt=/tmp/short.txt
echo "Rust is cool!" > "$short_txt"
mcopy -i "$output_file" "$short_txt" ::
