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

Add VGA graphics:

```
$ make run GRAPHICS=on
```

### QEMU interaction

I tend to prefer
[`-nographic`](https://www.qemu.org/docs/master/system/qemu-manpage.html#hxtool-3)
mode (`GRAPHICS=off` in the Makefile, the default) because it is easier to
interact with the QEMU monitor and there isn't a distracting window. Once I have
actual graphics besides hello world VGA text I'll prefer the graphical window.

Keybindings:
- `-nographic` mode: <https://www.qemu.org/docs/master/system/mux-chardev.html>
  - Specifically, press `Ctrl-a h` to see help
- Graphical frontend: <https://www.qemu.org/docs/master/system/keys.html>
  - Specifically, press `Ctrl+Alt+2` to get to QEMU monitor, and `Ctrl+Alt+1` to go back to OS's VGA output
  - `Ctrl+Alt+g` to release captured mouse and keyboard

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

- Synchronization primitives
  - Wait queues: I think just queues wrapped by spinlocks holding processes to wake up when an event happens. Maybe wrapper around `InitCell`?. Useful for device drivers?
  - Mutex (not spinlock "mutex") that handles sleeping and waking
  - In the future we should disable preemption when spin locks are taken
- Multi-tasking (see resources below)
  - Call `run_scheduler` after every IRQ if some global variable `NEED_RESCHEDULE` is set. This can be set both when the timer function sees a task has run out of its time slice, and when a device interrupt wakes up a task (can't call scheduler in the interrupt context, but we can at the exit)
  - Create a wrapper around `InitCell`, or just something like `InitCell`, that holds a reference to a `Task` and sets its state to `ReadyToRun` when the cell value is set. This is what the shell task will use to wait.
  - Preemption: call `scheduler_tick` every tick, update time slices, handle unscheduling current task if there is another pending task and the current task hit its time slice
    - Once we have preemption, remove `run_scheduler` call in `idle_task_start`
    - Remove `run_scheduler` call in `naive_nth_prime`
  - Consider storing context explicitly in struct like xv6 does <https://github.com/mit-pdos/xv6-public/blob/master/swtch.S>. This makes it easier to manipulate during setup.
- VirtIO improvements:
  - Create a physically contiguous heap, or slab allocator, or something for virtio buffer requests so we don't waste an entire page per tiny allocation.
    - Ensure we are still satisfying any alignment requirements for buffers. Read the spec!
  - Locking: we need to lock writes (I think?), but we should be able to read from the queue without locking. This should be ergonomic. I don't necessarily want to bury a mutex deep in the code.
    - Investigate how Linux or other OS virtio drivers do locking
  - Ensure we don't accidentally reuse descriptors while we are waiting for a response from the device. Don't automatically just wrap around! This is what might require a mutex rather than just atomic integers?
  - I think there is a race condition with the interrupts with the current non-locking mechanism. Ensure that if there are concurrent writes while an interrupt, then an interrupt won't miss a read (e.g. there will at least be a followup interrupt)
  - Remember features we negotiate, and ensure we are accounting for the different features in the logic (especially around notifications)
- bitmap-alloc
  - Make page vs byte address part of the API b/c conversion is tricky and requires `div_ceil`. Newtypes/functions for both?
    - We could embrace `PhysAddr`, `PhysFrame`, `Size4KiB`, etc, but that would introduce dep on `x86_64` crate
- Make a simple shell that runs hard-coded program names (not separate processes yet! Just inline code on the current thread)
  - Integrate with multi-tasking. Make a new task for the thing being run, and the shell's task simply waits for the sub-task to complete.
  - Show register values, internal structures, etc
  - Have help be meaningful
  - Split up tests and have subcommands like `test all` (all can be optional) `test interrupts`, `test memory-mappings`, etc
  - Eventually replace using the serial device with vsock or virtio-console for speed. Maybe the primary interface to the OS could be a TUI, which would be super neat!
- IOAPIC: Throw an error if IOAPIC number assigned to twice
- Investigate if we should be doing `apic::end_of_interrupt` for handlers on their behalf or not.
  - Consider the timer when we do scheduling. I think we want to call EOI _before_ the end of the timer handler, because we will be calling `schedule()` and we will be off to a new process
    - This is what Linux does. It calls `schedule()` in the "exit" part of an interrupt handler. Maybe that is where we should put it: in the common handler routing after ACK'ing the interrupt.
  - Linux uses spin locks for each IRQ, as well as masking interrupts but telling the APIC it got the interrupt <https://www.oreilly.com/library/view/understanding-the-linux/0596005652/ch04s06.html>
  - <https://docs.kernel.org/core-api/genericirq.html> mentions that a generic handler is hard b/c of APIC , IO/APIC, etc ACKs, which is why `__do_IRQ` no longer exists
- Detect kernel stack overflows. Guard pages? Some other mechanism?
  - I need a huge stack for debug mode apparently. I was seeing stack overflows with a 4096 byte stack when running in debug mode, so I quadrupled it
- Try replacing bitmap allocator with a buddy allocator, perhaps itself implemented with multiple bitmaps <https://wiki.osdev.org/Page_Frame_Allocation>
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
  - virtio-blk interrupts work! Just a problem with RNG
- Read [QEMU Internals](https://airbus-seclab.github.io/qemu_blog/)
- Filesystem support
  - Example <https://github.com/rafalh/rust-fatfs>
  - <https://wiki.osdev.org/FAT>
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
(qemu) info pci
(qemu) info qtree
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
  - Explains why you need 3 descriptors per request (different permissions needed). Links to <https://stackoverflow.com/questions/52037482/qemu-virtio-blk-strange-restrictions>
- <https://github.com/mit-pdos/xv6-riscv/blob/f5b93ef12f7159f74f80f94729ee4faabe42c360/kernel/virtio_disk.c>
- <https://marz.utk.edu/my-courses/cosc562/virtio/block/>
  - <https://web.eecs.utk.edu/~smarz1/courses/cosc361/notes/virtio/>
  - <https://web.eecs.utk.edu/~smarz1/courses/cosc361/notes/blockio/>

#### Struct offsets

The VirtIO spec uses C structs to compute offsets. Here is a C program that shows how to compute these offsets:

```c
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>

struct virtio_blk_config {
	uint64_t capacity;
	uint32_t size_max;
	uint32_t seg_max;
	struct virtio_blk_geometry {
		uint16_t cylinders;
		uint8_t heads;
		uint8_t sectors;
	} geometry;
	uint32_t blk_size;
	struct virtio_blk_topology {
		// # of logical blocks per physical block (log2)
		uint8_t physical_block_exp;
		// offset of first aligned logical block
		uint8_t alignment_offset;
		// suggested minimum I/O size in blocks
		uint16_t min_io_size;
		// optimal (suggested maximum) I/O size in blocks
		uint32_t opt_io_size;
	} topology;
	uint8_t writeback;
	uint8_t unused0;
	uint16_t num_queues;
	uint32_t max_discard_sectors;
	uint32_t max_discard_seg;
	uint32_t discard_sector_alignment;
	uint32_t max_write_zeroes_sectors;
	uint32_t max_write_zeroes_seg;
	uint8_t write_zeroes_may_unmap;
	uint8_t unused1[3];
	uint32_t max_secure_erase_sectors;
	uint32_t max_secure_erase_seg;
	uint32_t secure_erase_sector_alignment;
};

int main(void)
{

	printf("offsetof(capacity) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, capacity)),
	printf("offsetof(size_max) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, size_max)),
	printf("offsetof(seg_max) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, seg_max)),
	printf("offsetof(geometry) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, geometry)),
	printf("offsetof(blk_size) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, blk_size)),
	printf("offsetof(topology) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, topology)),
	printf("offsetof(writeback) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, writeback)),
	printf("offsetof(unused0) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, unused0)),
	printf("offsetof(num_queues) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, num_queues)),
	printf("offsetof(max_discard_sectors) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, max_discard_sectors)),
	printf("offsetof(max_discard_seg) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, max_discard_seg)),
	printf("offsetof(discard_sector_alignment) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, discard_sector_alignment)),
	printf("offsetof(max_write_zeroes_sectors) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, max_write_zeroes_sectors)),
	printf("offsetof(max_write_zeroes_seg) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, max_write_zeroes_seg)),
	printf("offsetof(write_zeroes_may_unmap) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, write_zeroes_may_unmap)),
	printf("offsetof(max_secure_erase_sectors) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, max_secure_erase_sectors)),
	printf("offsetof(max_secure_erase_seg) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, max_secure_erase_seg)),
	printf("offsetof(secure_erase_sector_alignment) = 0x%02X\n", (long) offsetof(struct virtio_blk_config, secure_erase_sector_alignment)),

	printf("sizeof(struct virtio_blk_config) = 0x%02X\n", (long) sizeof(struct virtio_blk_config));

	exit(EXIT_SUCCESS);
}
```

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
- Great overview <https://www.kernel.org/doc/html/latest/core-api/genericirq.html>
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
- Linux source:
  - [`__schedule()`](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/sched/core.c#L6506)
  - [`context_switch()`](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/sched/core.c#L5255)
  - [`finish_task_switch()`](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/sched/core.c#L5143)
  - [`__kthread_create_on_node()` (main part of `kthread_create()`)](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/kthread.c#L414)
    - [`kthreadd()`, main worker function of the `kthreadd` task](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/kthread.c#L718)
    - [`create_kthread`](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/kthread.c#L399)
      - Note how it calls `kernel_thread` with [`kthread`](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/kthread.c#L330) and then passes in the `create` value as an arg! That is, it runs `kthread`.
      - We actually deal with `fn` and `fn_arg` below in `copy_thread`
    - [calls `kernel_thread()`](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/fork.c#L2732)
    - [`kernel_clone()`, primary kthread cloning function](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/fork.c#L2642)
    - [`copy_process()`](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/fork.c#L2012)
    - [`copy_thread()`](https://elixir.bootlin.com/linux/v6.3.2/source/arch/x86/kernel/process.c#L135)
    - [`kthread_frame_init`](https://elixir.bootlin.com/linux/v6.3.2/source/arch/x86/include/asm/switch_to.h#L77) sets up the frame
    - [x86_64 `ret_from_fork`](https://elixir.bootlin.com/linux/v6.3.2/source/arch/x86/entry/entry_64.S#L279)
      - This is where we call the passed in function w/ an arg, notably `kthread` with the kthread creation args:
        ```asm
        	testq	%rbx, %rbx			/* from kernel_thread? */
        	jnz	1f				/* kernel threads are uncommon */

                ...

        1:
        	/* kernel thread */
        	UNWIND_HINT_EMPTY
        	movq	%r12, %rdi
        	CALL_NOSPEC rbx
        ```
    - [`schedule_tail`, the first thing a forked thread must call](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/sched/core.c#L5230)
    - When the kthread is done we call [`do_exit`](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/exit.c#L805) and then [`do_task_dead`](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/sched/core.c#L6635)
- xv6
  - [`forkret()`, first think run by a process for the first time](https://github.com/IamAdiSri/xv6/blob/4cee212b832157fde3289f2088eb5a9d8713d777/proc.c#L406-L425)

### Clock sources and time

- <https://www.kernel.org/doc/Documentation/virtual/kvm/timekeeping.txt>
- <https://wiki.osdev.org/HPET>
- <https://wiki.osdev.org/Timer_Interrupt_Sources>
- <https://wiki.osdev.org/Time_And_Date>
- <https://blog.trailofbits.com/2019/10/03/tsc-frequency-for-all-better-profiling-and-benchmarking/>
- <https://en.wikipedia.org/wiki/Time_Stamp_Counter>
- <https://en.wikipedia.org/wiki/High_Precision_Event_Timer>
- <https://0xax.gitbooks.io/linux-insides/content/Timers/linux-timers-6.html>
- <https://stackoverflow.com/questions/51919219/determine-tsc-frequency-on-linux>
- <https://stackoverflow.com/questions/13772567/how-to-get-the-cpu-cycle-count-in-x86-64-from-c/51907627#51907627>

### Memory barriers

When performing memory-mapped device IO, it is often important to ensure that
your reads and writes are performed in the order you write them in code. Using
volatile reads/writes can ensure that the _compiler_ doesn't reorder them, but
the CPU may still reorder them.

- <https://www.kernel.org/doc/Documentation/memory-barriers.txt>
- <https://lwn.net/Articles/847481/>
- <https://doc.rust-lang.org/core/sync/atomic/fn.compiler_fence.html>
- <https://en.wikipedia.org/wiki/Memory_barrier>

### Memory management

- <https://wiki.osdev.org/Page_Frame_Allocation>
- <https://wiki.osdev.org/Brendan%27s_Memory_Management_Guide>
- <https://wiki.osdev.org/Writing_a_memory_manager>
- <https://wiki.osdev.org/Memory_management>
- <https://forum.osdev.org/viewtopic.php?t=46327&p=327049>
- <https://eatplayhate.me/2010/09/04/memory-management-from-the-ground-up-2-foundations/>
- <https://en.wikipedia.org/wiki/Buddy_memory_allocation>
