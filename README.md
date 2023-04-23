# Rust OS

Inspired by [Writing an OS in Rust](https://os.phil-opp.com/) and <https://github.com/mrgian/felix>.

## Running

```
$ cargo bootimage
$ qemu-system-x86_64 -drive format=raw,file=target/x86_64-rust_os/debug/bootimage-rust-os.bin
```

## TODO

- <https://os.phil-opp.com/testing/>
- Add CI
- Use GRUB or make my own bootloader instead of using `bootloader` crate
  - QEMU can launch multiboot-compatible kernels directly, or we can use GRUB
  - <https://github.com/cirosantilli/x86-bare-metal-examples/tree/dbbed23e4753320aff59bed7d252fb98ef57832f/multiboot>
    - In general this repo is awesome <https://github.com/cirosantilli/x86-bare-metal-examples>
  - <https://wiki.osdev.org/Bare_Bones>
