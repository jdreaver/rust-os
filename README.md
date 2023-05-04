# Rust OS

Inspired by [Writing an OS in Rust](https://os.phil-opp.com/) and <https://github.com/mrgian/felix>.

## Running in QEMU

Default debug mode:

```
$ make run
```

Release mode:

```
$ make run RUST_BUILD_MODE=release
```


## Debugging with GDB

In one terminal:

```
make run-debug
```

In another

```
make gdb
```

## Tests

Note that we don't use a Cargo workspace (I turned it off because LSP/Emacs
didn't seem to work well under it, and it isn't clear how cargo configs should
work across workspaces, e.g. <https://github.com/rust-lang/cargo/issues/7004>
and <https://rustwiki.org/en/cargo/reference/config.html>). That means `cargo
test` doesn't work at the top level. Also, we don't use Rust's testing library
for kernel code. Instead we do more integration-style tests. Pure code that
_can_ be tested is put in a separate crate and those use Rust's test system.

```
make test
```

## TODO

- Filesystem support
  - Example <https://github.com/rafalh/rust-fatfs>
  - <https://wiki.osdev.org/FAT>
  - ATA
    - <https://wiki.osdev.org/ATA_PIO_Mode>
    - <https://wiki.osdev.org/ATA_read/write_sectors>
    - <https://github.com/mit-pdos/xv6-public/blob/master/ide.c>
  - Virtio, since we are running in QEMU anyway
    - <https://wiki.osdev.org/Virtio>
    - <https://www.qemu.org/2021/01/19/virtio-blk-scsi-configuration/>
    - <https://brennan.io/2020/03/22/sos-block-device/>
    - <https://wiki.osdev.org/PCI>
    - <https://github.com/mit-pdos/xv6-riscv/blob/f5b93ef12f7159f74f80f94729ee4faabe42c360/kernel/virtio_disk.c>
- Allocator designs <https://os.phil-opp.com/allocator-designs/>
- UEFI. I have it kind of set up, but I should poke at it more, and also investigate the Limine UEFI system table stuff
- Tests
  - <https://www.infinyon.com/blog/2021/04/rust-custom-test-harness/>
  - Useful resource, but I couldn't get this to work with the staticlib setup <https://os.phil-opp.com/testing/>
    - Try again with `main.rs`/ELF thanks to limine!
    - Might be useful <https://blog.frankel.ch/different-test-scopes-rust/>
    - Don't integrate with `cargo test`. Do `cargo build --tests` and have a `make test` target
  - Things to test:
    - Interrupts work (e.g. breakpoint).
      - Ensure breakpoint handler is called and that we resume
      - Ensure that fatal handlers like general protection fault are called then exit
    - Panic works (can exit with success after panic)
    - Double fault handlers work (e.g. stack overflow of kernel stack calls double fault handler)
    - Heap allocated memory, especially deallocation (create a ton of objects larger than the heap in a loop, which ensures that deallocation is happening or we would run out of memory)
- Unit tests for memory management, allocator, etc. Move to a new crate?
- Add CI
  - Check out <https://github.com/phil-opp/blog_os/blob/post-12/.github/workflows/code.yml>
  - Consider using nix to load dependencies
