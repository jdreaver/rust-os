# Rust OS

Inspired by [Writing an OS in Rust](https://os.phil-opp.com/) and <https://github.com/mrgian/felix>.

## Running

```
$ cargo bootimage
$ qemu-system-x86_64 -drive format=raw,file=target/x86_64-rust_os/debug/bootimage-rust-os.bin
```

## TODO

- GRUB2:
  - Actually parse the info given to use from GRUB2/multiboot, particularly for stack space, memory regions, etc
- <https://os.phil-opp.com/testing/>
- Add CI
