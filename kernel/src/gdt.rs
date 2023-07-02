//! Global Descriptor Table (GDT) and Task State Segment (TSS) setup.
//!
//! All memory accesses pass through the Global DescriptorTable (GDT). This
//! table holds segment descriptors that provide location and access bits for
//! different memory segments. We use a flat memory model, so segments cover the
//! entire address space, and the GDT is used to delineate between kernel and
//! user code.
//!
//! Also, in order to use the `syscall`/`sysret` instructions, we need to set up
//! the STAR register to tell those instructions which segments to use, and they
//! expect a very specific layout of the GDT. See
//! <https://wiki.osdev.org/GDT_Tutorial> and
//! <https://wiki.osdev.org/SYSENTER#AMD:_SYSCALL.2FSYSRET> for more details.
//!
//! See "2.1.1 Global and Local Descriptor Tables" in the Intel SDM Vol 3 and
//! <https://wiki.osdev.org/Global_Descriptor_Table>.

use core::mem::MaybeUninit;

use x86_64::instructions::tables::load_tss;
use x86_64::registers::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

use crate::apic::ProcessorID;
use crate::percpu;

// TODO: Create a percpu abstraction to do this more ergonomically and also
// ensure these are cache aligned. The problem is this needs to be created
// before the percpu machinery is initialized.
static mut PER_CPU_GDT: [MaybeUninit<GlobalDescriptorTable>; percpu::MAX_CPUS as usize] =
    MaybeUninit::uninit_array();

fn init_gdt(tss: &'static TaskStateSegment) -> GlobalDescriptorTable {
    let mut gdt = GlobalDescriptorTable::new();
    let kernel_code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
    let kernel_data_selector = gdt.add_entry(Descriptor::kernel_data_segment());

    // User code/data segments in 64 bit mode are here just to set the
    // privilege level to ring 3.
    //
    // N.B. The ordering here matters for some reason when we set the STAR
    // register. The user data segment must be added _before_ the user code
    // segment.
    let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
    let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());

    let tss_selector = gdt.add_entry(Descriptor::tss_segment(tss));

    assert_eq!(kernel_code_selector, KERNEL_CODE_SELECTOR);
    assert_eq!(kernel_data_selector, KERNEL_DATA_SELECTOR);
    assert_eq!(user_data_selector, USER_DATA_SELECTOR);
    assert_eq!(user_code_selector, USER_CODE_SELECTOR);
    assert_eq!(tss_selector, TSS_SELECTOR);

    gdt
}

// Hard-code these as consts so we can use them in assembly easier.

pub(crate) const KERNEL_CODE_SELECTOR: SegmentSelector =
    SegmentSelector::new(1, x86_64::PrivilegeLevel::Ring0);
pub(crate) const KERNEL_DATA_SELECTOR: SegmentSelector =
    SegmentSelector::new(2, x86_64::PrivilegeLevel::Ring0);
pub(crate) const USER_DATA_SELECTOR: SegmentSelector =
    SegmentSelector::new(3, x86_64::PrivilegeLevel::Ring3);
pub(crate) const USER_CODE_SELECTOR: SegmentSelector =
    SegmentSelector::new(4, x86_64::PrivilegeLevel::Ring3);
pub(crate) const TSS_SELECTOR: SegmentSelector =
    SegmentSelector::new(5, x86_64::PrivilegeLevel::Ring0);

pub(crate) const DOUBLE_FAULT_IST_INDEX: u16 = 0;
pub(crate) const PAGE_FAULT_IST_INDEX: u16 = 1;

/// How large to make the various TSS stacks.
const TSS_STACK_SIZE_BYTES: usize = 4096 * 5; // TODO: Is this too large?

// Statically allocate a bunch of the TSS stacks per CPU so we don't share them.
//
// TODO: Allocate these on the fly instead of hard coding them. GDT init
// generally runs before the physical allocator works. We can either use a
// bootstrap GDT with static stacks and then use the allocator for all of the
// per CPU stacks, or we can use something like a memblock allocator in early
// init: https://0xax.gitbooks.io/linux-insides/content/MM/linux-mm-1.html

static mut DOUBLE_FAULT_STACK_TABLES: [[u8; TSS_STACK_SIZE_BYTES]; percpu::MAX_CPUS as usize] =
    [[0; TSS_STACK_SIZE_BYTES]; percpu::MAX_CPUS as usize];

static mut PAGE_FAULT_STACK_TABLES: [[u8; TSS_STACK_SIZE_BYTES]; percpu::MAX_CPUS as usize] =
    [[0; TSS_STACK_SIZE_BYTES]; percpu::MAX_CPUS as usize];

fn get_tss_stack_ptr(
    processor_id: ProcessorID,
    stacks: &mut [[u8; TSS_STACK_SIZE_BYTES]; percpu::MAX_CPUS as usize],
) -> VirtAddr {
    let stack_start = VirtAddr::from_ptr(stacks[processor_id.0 as usize].as_ptr());
    #[allow(clippy::let_and_return)]
    let stack_end = stack_start + TSS_STACK_SIZE_BYTES;
    stack_end
}

// TODO: Create a percpu abstraction to do this more ergonomically and also
// ensure these are cache aligned. The problem is this needs to be created
// before the percpu machinery is initialized.
static mut PER_CPU_TSS: [TaskStateSegment; percpu::MAX_CPUS as usize] =
    [TaskStateSegment::new(); percpu::MAX_CPUS as usize];

fn create_tss(processor_id: ProcessorID) -> TaskStateSegment {
    // N.B. TSS is mostly used in 32 bit mode, but in 64 bit mode it is still
    // used for stack switching for fault handlers and for reserved stacks when
    // the CPU switches privilege levels. For double faults, it is important we
    // have a fresh stack so we can recover from a fault caused by a stack
    // overflow. Without a fresh stack, the CPU would try to allocate a stack
    // frame for the double fault handler and it would fail, causing a triple
    // fault.
    let mut tss = TaskStateSegment::new();

    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] =
        unsafe { get_tss_stack_ptr(processor_id, &mut DOUBLE_FAULT_STACK_TABLES) };

    tss.interrupt_stack_table[PAGE_FAULT_IST_INDEX as usize] =
        unsafe { get_tss_stack_ptr(processor_id, &mut PAGE_FAULT_STACK_TABLES) };

    tss
}

pub(crate) fn init_per_cpu_gdt(processor_id: ProcessorID) {
    let tss: &'static mut TaskStateSegment = unsafe { &mut PER_CPU_TSS[processor_id.0 as usize] };
    *tss = create_tss(processor_id);

    let gdt: &'static mut MaybeUninit<GlobalDescriptorTable> =
        unsafe { &mut PER_CPU_GDT[processor_id.0 as usize] };
    gdt.write(init_gdt(tss));

    unsafe {
        gdt.assume_init_mut().load();
    };

    init_segment_selectors();
}

fn init_segment_selectors() {
    unsafe {
        // Reload to the CS (code segment) and DS (data segment) registers to
        // point to the new GDT, not the GDT we built for bootstrapping.
        CS::set_reg(KERNEL_CODE_SELECTOR);
        DS::set_reg(KERNEL_DATA_SELECTOR);
        load_tss(TSS_SELECTOR);

        // NOTE: It is very important that the legacy data segment registers
        // (ES, FS, GS, SS) are set to zero. If they are not, the CPU will
        // try to access the GDT to get the segment descriptors for those
        // registers, but the GDT is not set up yet. This will cause a triple
        // fault.
        //
        // In 64 bit mode you don't need an actual data segment; using the null
        // segment from the GDT is okay. Many instructions, including iretq
        // (returning from exception handlers) require a data segment descriptor
        // _or_ the null descriptor.
        ES::set_reg(SegmentSelector(0));
        FS::set_reg(SegmentSelector(0));
        GS::set_reg(SegmentSelector(0));
        SS::set_reg(SegmentSelector(0));
    }

    // Use STAR to set the kernel and userspace segment selectors for the
    // SYSCALL and SYSRET instructions.
    x86_64::registers::model_specific::Star::write(
        USER_CODE_SELECTOR,
        USER_DATA_SELECTOR,
        KERNEL_CODE_SELECTOR,
        KERNEL_DATA_SELECTOR,
    )
    .unwrap_or_else(|err| panic!("Failed to set STAR: {err}"));
}

/// The TSS RSP0 is used when switching to kernel (ring 0 == RSP0) stack on
/// privilege level change. It needs to be set to the kernel stack for the task
/// currently running on the CPU.
pub(crate) fn set_tss_rsp0(processor_id: ProcessorID, rsp0: VirtAddr) {
    unsafe {
        let tss: &'static mut TaskStateSegment = &mut PER_CPU_TSS[processor_id.0 as usize];
        tss.privilege_stack_table[0] = rsp0;
    }
}
