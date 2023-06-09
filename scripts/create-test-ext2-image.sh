#!/usr/bin/env bash

set -eu

if [ "$#" -ne 1 ]; then
  echo "Usage: $0 OUTPUT_FILE" >&2
  exit 1
fi

cd "$(dirname "$0")/.."

output_file="$1"

rm -f "$output_file"
truncate -s 16M "$output_file"
mkfs.ext2 "$output_file"

# Mount the image to a temporary directory.
mount_dir=/tmp/ext2-test-image-mount
rm -rf "$mount_dir"
mkdir -p "$mount_dir"
sudo mount -oloop "$output_file" "$mount_dir"
user=$USER
sudo chown "$user" "$mount_dir"

# Populate some files
echo "Hello, world!" > "$mount_dir/hello.txt"
mkdir "$mount_dir/nested-dir"
echo "Nested hello" > "$mount_dir/nested-dir/nested.txt"

# Include userspace files
mkdir "$mount_dir/bin"

make -C userspace/hello clean
make -C userspace/hello
cp userspace/hello/hello "$mount_dir/bin/hello"

make -C userspace/primes clean
make -C userspace/primes
cp userspace/primes/primes "$mount_dir/bin/primes"

# Unmount
sudo exa --tree -lahgnimuU "$mount_dir"
sync "$mount_dir"
sudo umount "$mount_dir"
rm -rf "$mount_dir"
