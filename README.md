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

## Debugging kernel with GDB

In one terminal:

```
make run-debug
```

In another terminal:

```
make gdb
```

Make sure to read resource section below on using GDB with QEMU! In particular,
use `hbreak` instead of `break` to set a breakpoint before the kernel starts and
has page tables set up.

## Debugging QEMU with GDB

If you want to debug QEMU itself with GDB, you can run:

```
make run RUN_QEMU_GDB=yes
```

This can be very useful if you want to figure out why QEMU is doing something
funky. (I originally created this to debug why QEMU reported `Invalid write at
addr 0xFEE00000, size 4, region '(null)', reason: rejected` whenever MSI-X tried
to write an interrupt to `0xFEE00000` only in legacy boot mode, but not in UEFI
mode.)

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

- Multi-tasking (see resources below)
- Detect kernel stack overflows. Guard pages? Some other mechanism?
  - I need a huge stack for debug mode apparently. I was seeing stack overflows with a 4096 byte stack when running in debug mode, so I quadrupled it
- HPET for timing (apparently better than Local APIC timer?)
- VirtIO improvements:
  - Locking: we need to lock writes (I think?), but we should be able to read from the queue without locking. This should be ergonomic. I don't necessarily want to bury a mutex deep in the code.
    - Investigate how Linux or other OS virtio drivers do locking
  - Ensure we don't accidentally reuse descriptors while we are waiting for a response from the device. Don't automatically just wrap around! This is what might require a mutex rather than just atomic integers?
  - I think there is a race condition with the interrupts with the current non-locking mechanism. Ensure that if there are concurrent writes while an interrupt, then an interrupt won't miss a read (e.g. there will at least be a followup interrupt)
- `registers.rs` and macros
  - Consider moving `registers.rs` stuff into dedicated crate with unit tests
  - Also document `registers.rs` stuff
  - Consider using a proc macro to annotate fields on structs instead of
    code-generating the entire struct. This might get rid of the need for my PCI
    wrapper types because I can inline helper functions into the actual struct,
    and I can also include other information in the struct. Perhaps we can also
    handle the "embedding" case, where e.g. we want to easily be able to include
    the common PCI registers in a Type0 device struct, or allow VirtIO devices
    to include the common registers and the type 0 registers. Also, I think proc
    macros are more flexible.
    - Easier to set pub, private, pub(crate), etc
    - This would also allow us to group registers into dedicated structs. For
      example, have a struct for grouping (device_id, vendor_id), and maybe
      class, subclass, and prog_if. There really is no strict need to have all
      registers in the same struct generated through one macro, except it makes
      having a single `from_address` function and perhaps a `Debug`
      implementation nicer.
    - Find a way to use this macro for the virtq stuff, where the rings have dynamic size and then a struct member after that.
  - I really messed up my pointer math on some structs and now I'm scared. It
    would be _really_ nice to be able to rely on `#[repr(C)]` alignment rules,
    especially for VirtIO where they use C structs in the spec.
- virtio-rng interrupt doesn't seem to fire with UEFI disabled (`make run UEFI=off`). Fix it.
- Read [QEMU Internals](https://airbus-seclab.github.io/qemu_blog/)
- Filesystem support
  - Now that I have PCI working, attach a drive via QEMU and see what is looks like under PCI
    - I'm pretty sure there is just one SATA controller for multiple drives
  - Example <https://github.com/rafalh/rust-fatfs>
  - <https://wiki.osdev.org/FAT>
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

### QEMU

In the QEMU monitor (Ctrl+Alt+2 in a QEMU graphical window), these are useful
for looking at devices:

```
$ info pci
$ info qtree
```

Finding QEMU device help

```
# List devices
$ qemu-system-x86_64 -device help

# Help for a specific device
$ qemu-system-x86_64 -device virtio-rng-pci,help
```

- <https://marz.utk.edu/my-courses/cosc562/qemu/>

### GDB/Debugging

- Using `break` in GDB doesn't work when QEMU first starts because the kernel has a higher-half mapping, and the addresses aren't mapped yet. Instead, use `hbreak`.
  - <https://forum.osdev.org/viewtopic.php?f=13&t=39998>
- <https://airbus-seclab.github.io/qemu_blog/brk.html>
- <https://qemu-project.gitlab.io/qemu/system/gdb.html>

### Rust OS dev

- Excellent documentation. Goes well beyond the Blog OS stuff <https://github.com/bendudson/EuraliOS>
- <https://github.com/vinc/moros>
- <https://osblog.stephenmarz.com/index.html>
- <https://github.com/thepowersgang/rust_os>
- <https://poplar.isaacwoods.dev/book/>
  - <https://github.com/IsaacWoods/poplar>

### APIC and IO/APIC

- <https://wiki.osdev.org/APIC>
- APIC for keyboard interrupts (deal with ISA override) <https://www.reddit.com/r/osdev/comments/iipoqt/how_to_get_ioapic_handle_keyboard_interrupts/>
- <https://blog.wesleyac.com/posts/ioapic-interrupts>

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
- <https://marz.utk.edu/my-courses/cosc562/pcie/>
- [What do the different interrupts in PCIe do? I referring to MSI, MSI-X and INTx](https://electronics.stackexchange.com/questions/76867/what-do-the-different-interrupts-in-pcie-do-i-referring-to-msi-msi-x-and-intx/)

### Virtio

- Spec: <https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.pdf>
- <https://wiki.osdev.org/Virtio>
- <https://blogs.oracle.com/linux/post/introduction-to-virtio>
- <https://wiki.libvirt.org/Virtio.html>
- <https://airbus-seclab.github.io/qemu_blog/regions.html> (show virtio regions with `info mtree`)
- <https://marz.utk.edu/my-courses/cosc562/virtio/rng/>

Block device:
- <https://www.qemu.org/2021/01/19/virtio-blk-scsi-configuration/>
- <https://brennan.io/2020/03/22/sos-block-device/>
- <https://github.com/mit-pdos/xv6-riscv/blob/f5b93ef12f7159f74f80f94729ee4faabe42c360/kernel/virtio_disk.c>
- <https://marz.utk.edu/my-courses/cosc562/virtio/block/>

### Volatile memory access in Rust, and spurious reads

TL;DR: Use raw pointers instead of references to memory-mapped IO regions to
guarantee you won't have spurious reads. There is an excellent [blog
post](https://lokathor.github.io/volatile/) that explains this. There is also a
good [forum
thread](https://users.rust-lang.org/t/how-to-make-an-access-volatile-without-std-library/85533/)
explaining the dangers of this.

### Interrupt/IRQ handling

- <https://unix.stackexchange.com/questions/47306/how-does-the-linux-kernel-handle-shared-irqs>
- <https://www.kernel.org/doc/html/next/PCI/msi-howto.html>
- <https://www.oreilly.com/library/view/linux-device-drivers/0596005903/ch10.html>

How linux does things:
- For CPU exceptions (vectors < 32), they have a hard-coded handler in the IDT
- For external interrupts (starting at 32) Linux pre-populates a stub interrupt handler for every vector (256 - 32 of them on x86_64) that simply calls `common_interrupt` with the vector number.
  - [This is the code](https://elixir.bootlin.com/linux/v6.3/source/arch/x86/include/asm/idtentry.h#L483) where they create the stubs
  - [`DECLARE_IDTENTRY` definition](https://elixir.bootlin.com/linux/v6.3/source/arch/x86/include/asm/idtentry.h#L17), which [is used](https://elixir.bootlin.com/linux/v6.3/source/arch/x86/include/asm/idtentry.h#L636) (via one intermediate macro in the same file) to create `asm_common_interrupt`, which is what the stub jumps to.
- [Definition for `common_interrupt`](https://elixir.bootlin.com/linux/v6.3/source/arch/x86/kernel/irq.c#L240)
  - [`DEFINE_IDTENTRY_IRQ` def](https://elixir.bootlin.com/linux/v6.3/source/arch/x86/include/asm/idtentry.h#L191)

Other higher-level Linux resources:
- <https://github.com/torvalds/linux/blob/bb7c241fae6228e89c0286ffd6f249b3b0dea225/arch/x86/include/asm/irq_vectors.h>
  - They _statically_ define what each IDT entry will do (though some are generic, like 32..127 being for device interrupts)
  - `SPURIOUS_APIC_VECTOR = 0xff`, they do this too <https://github.com/torvalds/linux/blob/bb7c241fae6228e89c0286ffd6f249b3b0dea225/arch/x86/include/asm/irq_vectors.h#L53-L61>
- <https://subscription.packtpub.com/book/iot-and-hardware/9781789342048/2/ch02lvl1sec06/linux-kernel-interrupt-management>
- <https://linux-kernel-labs.github.io/refs/heads/master/lectures/interrupts.html>
- <http://books.gigatux.nl/mirror/kerneldevelopment/0672327201/ch06lev1sec6.html>
- <https://0xax.gitbooks.io/linux-insides/content/Interrupts/linux-interrupts-8.html>

### Multi-tasking and context switching

- <https://github.com/bendudson/EuraliOS/blob/main/doc/journal/01-interrupts-processes.org>
- Excellent series of videos:
  - [Thread implementation #4 - Context switch | cs370](https://www.youtube.com/watch?v=YY2VXuaLBVc)
  - [Thread implementation #5 - Linux context switch | cs370](https://www.youtube.com/watch?v=3gOk3-X4y2U)
- <https://wiki.osdev.org/Brendan%27s_Multi-tasking_Tutorial>
- <https://www.reddit.com/r/osdev/comments/jf1wgy/multitasking_tutorial/>
- <https://wiki.osdev.org/Context_Switching>
- <https://wiki.osdev.org/Kernel_Multitasking>
- <https://samwho.dev/blog/context-switching-on-x86/>
  - Video that goes over this same xv6 code: [Operating Systems Lecture 25: Context switching in xv6](https://www.youtube.com/watch?v=fEnWqibCwo0)
- <https://stackoverflow.com/questions/12630214/context-switch-internals>
- Excellent history of `switch_to` in Linux over the years <https://www.maizure.org/projects/evolution_x86_context_switch_linux/>
