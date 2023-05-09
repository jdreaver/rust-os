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

UEFI disabled (use BIOS):

```
$ make run UEFI=off
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

- VirtIO
  - Get RNG (Entropy) device working
  - Figure out PCI interrupts (MSI-X?)
- PCI enums:
  - Have a wrapper object that does all the necessary inspection of bits (e.g. header_type to decide on body type 0/1, or vendor_id to decide on VirtIO device type) and returns an enum.
  - VirtIO device can check for vendor_id, decide it is VirtIO, assert type 0 body, and then call another function to get a more specific device.
  - The more specific type wraps sub types. For example, type 0 device wraps header and type 0 body. VirtIO device wraps header, body, and VirtIO specific stuff
    - Instead of going from common config -> config -> VirtIO, consider having VirtIO accepting a common config (or even a location!) and trying to get at the type 0 config
    - Wrapper objects should contain multiple `register_struct` structs. Don't try nesting them.
    - Figure out printing. Currently it is top-down, which won't work. Make separate print functions?
  - Leaf level VirtIO objects can pre-parse their capabilities and store pointers to important ones, like the common config (and error if there are multiple?)
    - Alternatively, they could alloc a `Vec` per capability type.
- Consider moving `registers.rs` stuff into dedicated crate with unit tests
  - Also document `registers.rs` stuff
- Read [QEMU Internals](https://airbus-seclab.github.io/qemu_blog/)
- Filesystem support
  - Now that I have PCI working, attach a drive via QEMU and see what is looks like under PCI
    - I'm pretty sure there is just one SATA controller for multiple drives
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
- Serial print deadlock during interrupt: if we hit an interrupt while we are in
  the middle of printing to the serial port, and the interrupt needs to print to
  the serial port, we can deadlock.
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


## Resources

### PCI

- Spec <https://picture.iczhiku.com/resource/eetop/SYkDTqhOLhpUTnMx.pdf>
- <https://wiki.osdev.org/PCI>
- <https://wiki.osdev.org/PCI_Express>
- <https://tldp.org/LDP/tlk/dd/pci.html>
- Rust code/crate: <https://docs.rs/pci-driver/latest/pci_driver/>
- Great example Rust code <https://gitlab.com/robigalia/pci/-/blob/master/src/lib.rs>
  - I think this is the crate but it is old <https://docs.rs/pci/latest/pci/>
- <https://marz.utk.edu/my-courses/cosc562/pcie/>
- BAR memory mapping
  - <https://stackoverflow.com/questions/20901221/pci-express-bar-memory-mapping-basic-understanding>
  - <https://superuser.com/questions/746458/pci-bar-memory-addresses>
  - <https://softwareengineering.stackexchange.com/questions/358817/how-does-the-base-address-registers-bars-in-a-pci-card-work>

### Virtio

- Spec: <https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.pdf>
- <https://wiki.osdev.org/Virtio>
- <https://blogs.oracle.com/linux/post/introduction-to-virtio>
- <https://wiki.libvirt.org/Virtio.html>
- <https://airbus-seclab.github.io/qemu_blog/regions.html> (show virtio regions with `info mtree`)

### Volatile memory access in Rust, and spurious reads

TL;DR: Use raw pointers instead of references to memory-mapped IO regions to
guarantee you won't have spurious reads. There is an excellent [blog
post](https://lokathor.github.io/volatile/) that explains this. There is also a
good [forum
thread](https://users.rust-lang.org/t/how-to-make-an-access-volatile-without-std-library/85533/)
explaining the dangers of this.
