# Rust OS

Inspired by [Writing an OS in Rust](https://os.phil-opp.com/) and <https://github.com/mrgian/felix>.

## Running

```
$ make run
```

## TODO

- Paging implementation
  - Version 2 of tutorial <https://os.phil-opp.com/paging-implementation/>
  - Version 1 <https://os.phil-opp.com/allocating-frames/>
  - Linux kernel does linear mapping. Could just do that.
  - Consider using limine again
- Print text using limine framebuffer
  - <https://wiki.osdev.org/VGA_Fonts>
  - <https://wiki.osdev.org/Drawing_In_a_Linear_Framebuffer>
  - Consider double buffering for speed
- Tests
  - <https://www.infinyon.com/blog/2021/04/rust-custom-test-harness/>
  - Useful resource, but I couldn't get this to work with the staticlib setup <https://os.phil-opp.com/testing/>
    - Try again with `main.rs`/ELF thanks to limine!
    - Might be useful <https://blog.frankel.ch/different-test-scopes-rust/>
    - Don't integrate with `cargo test`. Do `cargo build --tests` and have a `make test` target
  - Things to test:
    - Fault handlers work (e.g. breakpoint)
    - Double fault handlers work (e.g. stack overflow of kernel stack calls double fault handler)
- Add CI
  - Check out <https://github.com/phil-opp/blog_os/blob/post-12/.github/workflows/code.yml>
  - Consider using nix to load dependencies
- Get limine installed as a flake package instead of a git submodule
