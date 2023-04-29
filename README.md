# Rust OS

Inspired by [Writing an OS in Rust](https://os.phil-opp.com/) and <https://github.com/mrgian/felix>.

## Running

```
$ cargo bootimage
$ qemu-system-x86_64 -drive format=raw,file=target/x86_64-rust_os/debug/bootimage-rust-os.bin
```

## TODO

- Paging implementation
  - Version 2 of tutorial <https://os.phil-opp.com/paging-implementation/>
  - Version 1 <https://os.phil-opp.com/allocating-frames/>
  - Linux kernel does linear mapping. Could just do that.
  - Consider using limine again
- Try limine again (via <https://github.com/jdreaver/rust-os/pull/1>) and use serial port for output/debugging until I implement writing bitmap text.
  - This might be really useful for better memory mapping support from the bootloader.

- Tests
  - <https://www.infinyon.com/blog/2021/04/rust-custom-test-harness/>
  - Useful resource, but I couldn't get this to work with the staticlib setup <https://os.phil-opp.com/testing/>
    - Might be useful <https://blog.frankel.ch/different-test-scopes-rust/>
    - Don't integrate with `cargo test`. Do `cargo build --tests` and have a `make test` target
  - Things to test:
    - Fault handlers work (e.g. breakpoint)
    - Double fault handlers work (e.g. stack overflow of kernel stack calls double fault handler)
- Add CI
  - Check out <https://github.com/phil-opp/blog_os/blob/post-12/.github/workflows/code.yml>
  - Consider using nix to load dependencies
