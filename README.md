# Rust OS

Inspired by [Writing an OS in Rust](https://os.phil-opp.com/) and <https://github.com/mrgian/felix>.

## Running

```
$ make run
```

## TODO

- Allocator designs <https://os.phil-opp.com/allocator-designs/>
- Print text using limine framebuffer
  - Put this in e.g. a `framebuffer` or `vesa_framebuffer` library under this workspace
    - If I parse psf fonts, make a `psf` crate too
  - Actual C array front:
    - <https://github.com/isometimes/rpi4-osdev/blob/master/part5-framebuffer/terminal.h>
    - <https://www.rpi4os.com/part5-framebuffer/#writing-characters-to-the-screen>
  - <https://wiki.osdev.org/VGA_Fonts>
  - <https://wiki.osdev.org/Drawing_In_a_Linear_Framebuffer>
  - <https://wiki.osdev.org/PC_Screen_Font>
  - <https://wiki.osdev.org/VESA_Video_Modes>
  - Consider double buffering for speed
  - <https://stackoverflow.com/questions/2156572/c-header-file-with-bitmapped-fonts>
  - <https://courses.cs.washington.edu/courses/cse457/98a/tech/OpenGL/font.c>
  - <https://jared.geek.nz/2014/jan/custom-fonts-for-microcontrollers>
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
