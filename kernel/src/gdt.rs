//! Global Descriptor Table (GDT) and Task State Segment (TSS) setup.

use lazy_static::lazy_static;
use x86_64::instructions::tables::load_tss;
use x86_64::registers::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let kernel_code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));

        // User code/data segments in 64 bit mode are here just to set the
        // privilege level to ring 3.
        //
        // N.B. The ordering here matters for some reason when we set the STAR
        // register. The user data segment must be added _before_ the user code
        // segment.
        let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
        let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());
        (
            gdt,
            Selectors {
                kernel_code_selector,
                kernel_data_selector,
                tss_selector,
                user_code_selector,
                user_data_selector,
            },
        )
    };
}

#[derive(Debug, Clone)]
pub(crate) struct Selectors {
    pub(crate) kernel_code_selector: SegmentSelector,
    pub(crate) kernel_data_selector: SegmentSelector,
    pub(crate) tss_selector: SegmentSelector,
    pub(crate) user_code_selector: SegmentSelector,
    pub(crate) user_data_selector: SegmentSelector,
}

pub(crate) fn selectors() -> Selectors {
    GDT.1.clone()
}

pub(crate) const DOUBLE_FAULT_IST_INDEX: u16 = 0;
pub(crate) const PAGE_FAULT_IST_INDEX: u16 = 1;

lazy_static! {
    // N.B. TSS is mostly used in 32 bit mode, but in 64 bit mode it is still
    // used for stack switching for fault handlers and for reserved stacks when
    // the CPU switches privilege levels. For double faults, it is important we
    // have a fresh stack so we can recover from a fault caused by a stack
    // overflow. Without a fresh stack, the CPU would try to allocate a stack
    // frame for the double fault handler and it would fail, causing a triple
    // fault.
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();

        // TODO: DRY setting up these TSS stacks.
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            let stack_start = VirtAddr::from_ptr(unsafe { &STACK });
            #[allow(clippy::let_and_return)]
            let stack_end = stack_start + STACK_SIZE;
            stack_end
        };
        tss.interrupt_stack_table[PAGE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            let stack_start = VirtAddr::from_ptr(unsafe { &STACK });
            #[allow(clippy::let_and_return)]
            let stack_end = stack_start + STACK_SIZE;
            stack_end
        };
        tss
    };
}

pub(crate) fn init(do_load_tss: bool) {
    GDT.0.load();
    unsafe {
        // Reload to the CS (code segment) and DS (data segment) registers to
        // point to the new GDT, not the GDT we built for bootstrapping.
        CS::set_reg(GDT.1.kernel_code_selector);
        DS::set_reg(GDT.1.kernel_data_selector);
        if do_load_tss {
            load_tss(GDT.1.tss_selector);
        }

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
        GDT.1.user_code_selector,
        GDT.1.user_data_selector,
        GDT.1.kernel_code_selector,
        GDT.1.kernel_data_selector,
    )
    .unwrap_or_else(|err| panic!("Failed to set STAR: {err}"));
}
