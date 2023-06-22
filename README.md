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

Provide command line arguments:

```
$ make run CMDLINE='hello world'
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

- Tests: Add thorough unit test suite we can trigger with shell command.
  - Consider combining all crates into kernel again now that we support tests
    - Make sure the bitmap-alloc proptest tests are still useful! Force a few failures. I'm a bit worried that proptest w/ no_std and panic == abort isn't useful
  - Have a way to run tests on boot and return the QEMU exit code with the result. Just short circuit to running tests instead of the shell.
    - Might need to encapsulate compilation + running in a shell script instead of having it in the Makefile, so that `make test` can modify the CMDLINE arguments. Or at the very least creating the image must be done in a script.
  - Integration tests:
    - Spawn a bunch of processes and hope we don't crash?
    - maybe some expected failures to ensure we call panic handler?
- Memory management
  - `Page` type improvements
    - Make typed page sizes like the x86_64 crate does
  - Add support for huge pages in `map_to`
  - Consider using some code in `test-x86-paging-performance` branch to simplify logic. I like some of it. <https://github.com/jdreaver/rust-os/compare/master...test-x86-paging-performance>
  - Test not zeroing leaf pages by default when allocating. (It is important to zero out intermediate tables that are created, but not the leaf pages).
  - Abandon the default limine memory mapping and make our own
    - Make sure to copy the pages relating to how the kernel is loaded though. Limine did all the hard work parsing the ELF file and set page permissions properly (or so I hope) for e.g. text, data, etc
  - Map all physical memory starting at `0xffff_8000_0000_0000`. Limine just does 4 GiB, but make sure to do it all.
  - Deal with freeing buffers used for mapping. We can't blindly deallocate every time we unmap because some mapping targets are device MMIO.
    - Perhaps don't allow the `NewPhysPage` mapping target. Or, make sure the caller uses the `PhysPage` result.
    - The kernel stack allocator actually has a bug where it doesn't free its allocated physical memory pages! It is only "freeing" the virtual pages.
  - Make our own `PhysAddr` and don't allow it to be converted to a pointer via `as_ptr()` (the x86_64 one doesn't have this btw)
  - Consider removing `as_u64` for all address types, because it makes mistakes too easy.
  - Guard pages: consider using one of the special OS-available bits on pages for `GUARD_PAGE`, in case that could simplify our guard page detection logic in the page fault handler. Using these OS-available bits in general to identify the type of page is probably going to be useful.
    - This will require not simply "unmapping" a page for the guard page, but to add some sort of "unmap with flags", or mapping "to" physical address 0 with flags (this is what we used to do)
  - Make it trivial to create a userspace page table.
    - Make kernel page table cloneable: fill entire top half (even if most level 3 page tables are empty), and zero out bottom half. That means we only use `KernelPhysAddr`.
      - Zeroing out bottom half means we likely don't want any sort of "identity map" function with `PhysAddr`
    - Since bottom half of kernel page table should be empty, after cloning we can fill in userspace segments into bottom half.
  - Once new page tables are in, re-examine visibility of all types and functions. Only expose what is needed out of `memory` (e.g. we probably don't need other modules touching raw page tables)
  - Linux prefers to use physical allocation in the kernel by default (kmalloc) because it is faster than virtual allocation (vmalloc) because vmalloc needs to mess with page tables. Vmalloc is only used when you need a huge chunk of memory that might be hard to get physically contiguous.
  - Linux keeps a 40 byte page struct per physical page of memory. That is way larger than my 1 bit! It only takes 40MB of a 4GB system (assuming 4kB pages) which might be an acceptable tradeoff.
  - Page table concurrency:
    - Consider representing each PageTableEntry as `AtomicU64`, or in the page table as `AtomicInt<u64, PageTableEntry>`
- Userspace
  - Set up and execute ELF for real in `task_userspace_setup`. Map segments to memory, make a stack, use real start location, etc.
    - Use a fresh page table!
      - Copy all of the higher half entries for the kernel page table.
      - Make a TODO to ensure this is robust. Perhaps we need at least some dummy entries for the higher half so when they _do_ get mapped all of the process page table higher halfs point to the same L3 tables
    - Drop any memory we allocated for task, like task segments
      - Also drop any intermediate page tables we created.
        - <https://docs.rs/x86_64/0.14.10/x86_64/structures/paging/mapper/trait.CleanUp.html#tymethod.clean_up_addr_range>
      - Would it be easier to create an arena holding a process's memory so we could drop it all?
  - Ensure that _every_ time we go to userspace, especially if we get rescheduled to another CPU, we store the kernel stack in the GS register. Do we need to add something to when we exit interrupt handlers, like a `return_to_userspace`?
  - Re-enable interrupts while handling syscalls (or don't? at least be explicit)
    - If we expect interrupts to be disabled, make a comment where we disabled and where we do e.g. `swapgs` or something else that expects interrupts disabled
  - Figure out how to get to userspace for the first time with sysretq instead of iretq
  - Define actual system calls
  - Segfault a user process and kill it instead of panicking and crashing the kernel
    - Be careful about locking the scheduler in the page fault handler. It is possible a spin lock was already taken on the scheduler and we'll deadlock (all though that shouldn't happen on the current CPU. Hmm)
  - Create a type showing the intended memory mapping of a process and turn that into a page table. This should make it easier to reason about the memory map.
- Ensure kernel pages are not marked as `USER_ACCESSIBLE`. I think the `x86_64` allocator, or limine, is doing it by default
- Per CPU
  - Maybe have a helper to take locks for multiple CPUs in a consistent way to prevent deadlocks, like ordering by processor ID. (Linux scheduler code does this for per CPU run queues)
  - Logging dependency on percpu:
    - Consider making init dependencies more explicit, by passing around a thing that was initialized, or even dumb tokens like `struct PerCPUInitialized;`.
    - Dep exists because the logger uses a spin lock, which modifies the percpu variable, logging depends on percpu being set up. Ensure percpu is set up before logging, or disable the logging spin locks until bootstrapping is done.
    - Maybe add debug_assert! calls ensuring percpu has been initialized before any percpu vars are used. (Make sure they get removed in release builds)
    - Linux has an early printk function, maybe for stuff like this?
    - Logging in already "slow" by kernel standards. Maybe it is okay to do some boolean check to take locks or not.
- Task start safety: find a way to make casting the `arg: *const ()` pointer way safer. It is easy to mess up.
- Arc memory leak detection:
  - Calling `run_scheduler()` (or more specifically `switch_to_task`) while holding an `Arc` reference (especially `Arc<Task>`) can cause a memory leak because we might switch away from the given task forever. Currently I manually `drop` things before calling these functions. Is there a way I could make calling `run_scheduler` basically impossible?
    - Same problem happens when jumping to userspace for the first time.
  - Find a way to detect leaked tasks, or maybe debug `Arc` leaks in general.
- Networking
- Filesystem
  - Writes
    - Instead of providing a block iterator function, have existing users request specific blocks. They can do iteration on their own. Then a user can request a new block at a specific location if it needs to do a write and a block doesn't exist.
    - Adding blocks to a file (maybe use some lorem ipsum generator or something to make up text of a given length, or embed some out-of-copyright literature in the binary)
      - Also ensure we add a block to directory inodes if we are trying to add a new directory entry and there isn't enough space. Might need a special constructor on `DirectoryEntry` for this case.
      - Make a function to find a new block given a desired block. It can be very dumb for now and in the future it can be smarter, but try to make a good interface for future improvements.
        - One simple but likely very effective algorithm is to first try and allocate a free block right after the previous block, and I'd that fails search for free blocks by byte in the bitmap (which would fine 8 free blocks in a row, and is super fast).
    - Appending: make sure we use the file length module block size to index into blocks. Don't just iterate over blocks.
    - Inode stats: ensure `blocks` and `size_low`/`size_high` fields are kept up to date
    - Creating a new file
  - Deletes (delete an entire file, unmark all inodes and blocks, etc)
  - Nested mountpoints, e.g. mount ext2 at root and then sysfs at `/sys`
    - Add mountpoint argument to `mount` and ensure parent directory exists (or mountpoint is `/`)
    - How do we ensure that when we do `ls /` we see `/sys`?
  - Ensure overwriting file properly truncates all blocks first by marking them as free and removing them from inode block pointers
  - Sysfs ideas: pci devices, virtio devices, memory info
  - Instead of returning `Vec` for directories, consider returning an `impl Iterator` (except you probably can't do that with traits...)
- GDB:
  - Add helpers for printing my common data structures better (`OnceCell`, `SpinLock`, `Mutex`, `BTReeMap`. Also third party like `SpinMutex`)
  - Also consider Emacs gdb window layout helper
- Serial port:
  - find a way to implement using `&mut` and locking without deadlocks from e.g. non-maskable interrupts, holding the lock in the shell while trying to debug print inside kernel code, etc.
  - Consider multiple serial ports: one that spits out logs from the kernel, and one dedicated to the shell.
- Spurious wakeups: refactor killing and sleeping so we don't rely on never having spurious wakeups, and so we don't need to rely on `&mut self` for scheduler to immediately run scheduler just once (we should run scheduler in a loop in case of spurious wakeup).
- ansiterm: move this to a separate crate with tests?
- Task struct access: investigate not hiding all tasks (or just the current tasks) inside the big scheduler lock. Are there situations where it is okay to modify a task if the scheduler is running concurrently? Can we lock individual tasks? Is this inviting a deadlock?
  - For example, putting a task to sleep or waking it up. Is this bad to do concurrently with the scheduler? Maybe instead of calling this the "state" it can be thought of as a "state intent", which the scheduler should action next time it changes the task's scheduling. Wait queues and channels do this, but they need a scheduler lock under the hood.
  - Consider using per CPU for storing the currently running task instead of having a Vec of those in `Scheduler`
    - The "current task" is only valid in the current thread. We need to wrap it in a type that is not `Send` or `Sync`; we can't just return a `&'static` ref (or maybe we can if we make sa
- Stack size: figure out why stacks need to be so large when compiling in debug mode. Is Rust putting a ton of debug info on the stack?
- Deadlock debugging: find a way to detect deadlocks and print the locks involved
  - Linux has a neat debugging system for mutexes <https://docs.kernel.org/locking/mutex-design.html#semantics>
  - Should we fail if we are holding a spinlock for too long?
  - Consider naming spinlocks, and having the lock holder put their name once they take the lock. Then if we fail we can dump all of this info.
- Consider storing task context explicitly in struct like xv6 does <https://github.com/mit-pdos/xv6-public/blob/master/swtch.S>. This makes it easier to manipulate during setup.
- Pre-process DWARF info to use for stack traces so we don't need to keep frame pointers around (we currently have `-C force-frame-pointers=yes`)
  - Do something like Linux's ORC <https://blogs.oracle.com/linux/post/unwinding-stack-frame-pointers-and-orc>
- Driver model:
  - Make a proper device and driver model like Linux. Then we can more easily populate sysfs, the PCI setup code can be less ad hoc (and more focused), mounting should be easier, etc.
  - Research kobject and sysfs
  - Linux has a simple match function on each driver on a bus. To find the right driver for a device, the bus iterates over all registered drivers and calls match, and matches to the first driver that returns 1.
- VirtIO improvements:
  - Create a physically contiguous heap, or slab allocator, or something for virtio buffer requests so we don't waste an entire page per tiny allocation.
    - Ensure we are still satisfying any alignment requirements for buffers. Read the spec!
  - Remember features we negotiate, and ensure we are accounting for the different features in the logic (especially around notifications)
- PCI device locking and `&mut` (and really locking anything that wraps registers)
  - Ensure modifying PCI devices requires a `&mut` reference to some actual "device" object. That means we shouldn't pass around raw registers. Something should be wrapping these.
- IOAPIC: Throw an error if IOAPIC number assigned to twice
- IRQ locking:
  - Linux uses spin locks for each IRQ, as well as masking interrupts but telling the APIC it got the interrupt <https://www.oreilly.com/library/view/understanding-the-linux/0596005652/ch04s06.html>
  - <https://docs.kernel.org/core-api/genericirq.html> mentions that a generic handler is hard b/c of APIC , IO/APIC, etc ACKs, which is why `__do_IRQ` no longer exists
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

- <https://sourceware.org/gdb/onlinedocs/gdb/Rust.html>
- <https://www.cse.unsw.edu.au/~learn/debugging/modules/gdb_init_file/>

In QEMU:
- Using `break` in GDB doesn't work when QEMU first starts because the kernel has a higher-half mapping, and the addresses aren't mapped yet. Instead, use `hbreak`.
  - <https://forum.osdev.org/viewtopic.php?f=13&t=39998>
- <https://airbus-seclab.github.io/qemu_blog/brk.html>
- <https://qemu-project.gitlab.io/qemu/system/gdb.html>

### Rust OS dev

- <https://github.com/Techno-coder/example_os>
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
- <https://elixir.bootlin.com/linux/v6.3.7/source/Documentation/x86/entry_64.rst>
- <https://github.com/torvalds/linux/blob/master/arch/x86/entry/entry_64.S>

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

### File systems

ext2:
- <https://wiki.osdev.org/Ext2>
- <https://www.nongnu.org/ext2-doc/ext2.html>
- <https://en.wikipedia.org/wiki/Ext2>
- <https://git.kernel.org/pub/scm/utils/util-linux/util-linux.git/tree/libblkid/src/superblocks/ext.c>
- "CHAPTER 18: The Ext2 and Ext3 Filesystems" in "Understanding the Linux Kernel - Bovet (3rd ed, 2005)"
- "Ch 9, The Extended Filesystem Family" in "Professional Linux Kernel Architecture - Maurer (2008)"

Linux block devices:
- <https://linux-kernel-labs.github.io/refs/heads/master/labs/block_device_drivers.html>
- [https://lwn.net/Articles/27055/](https://lwn.net/Articles/27055/)
- Chapter 16, Block Drivers in "Linux Device Drivers - Corbet, Koah-Hartman (3rd ed, 2005)"

Linux VFS:
- <https://www.kernel.org/doc/html/next/filesystems/vfs.html>
- <https://tldp.org/LDP/khg/HyperNews/get/fs/vfstour.html>
- Chapter 13 and 14 of "Linux Kernel Development - Love (3rd ed, 2010)"

### Userspace and system calls in x86_64

- <https://blog.llandsmeer.com/tech/2019/07/21/uefi-x64-userland.html>
- <https://nfil.dev/kernel/rust/coding/rust-kernel-to-userspace-and-back/>
- <https://github.com/bendudson/EuraliOS/blob/main/doc/journal/02-userspace.org>
- <https://wiki.osdev.org/System_Calls>
- <https://wiki.osdev.org/Sysenter> (also discusses syscall)
- <https://wiki.osdev.org/SWAPGS>

### Per CPU variables

- <https://docs.kernel.org/core-api/this_cpu_ops.html>
- <https://elixir.bootlin.com/linux/latest/source/include/linux/percpu.h>
- <https://elixir.bootlin.com/linux/latest/source/arch/x86/include/asm/percpu.h>
- Useful info about `swapgs`:
  - <https://elixir.bootlin.com/linux/v6.3.7/source/Documentation/x86/entry_64.rst>
  - <https://elixir.bootlin.com/linux/v6.3.7/source/arch/x86/entry/entry_64.S#L1054>
- `PERCPU_VADDR` is references in the x86 linker script to set up percpu area and make offsets look zero-based from start of percpu region
  - <https://elixir.bootlin.com/linux/v6.3.7/source/include/asm-generic/vmlinux.lds.h#L1067>
  - <https://elixir.bootlin.com/linux/v6.3.7/source/arch/x86/kernel/vmlinux.lds.S#L223>
