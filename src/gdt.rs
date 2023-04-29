//! Global Descriptor Table (GDT) and Task State Segment (TSS) setup.

use lazy_static::lazy_static;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));
        (
            gdt,
            Selectors {
                code_selector,
                tss_selector,
            },
        )
    };
}

struct Selectors {
    code_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

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
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
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

pub fn init() {
    use x86_64::instructions::tables::load_tss;
    use x86_64::registers::segmentation::{Segment, CS, DS, ES, FS, GS, SS};

    GDT.0.load();
    unsafe {
        // Reload to the CS (code segment) register to point to the new GDT, not
        // the GDT we built for bootstrapping.
        CS::set_reg(GDT.1.code_selector);
        load_tss(GDT.1.tss_selector);

        // NOTE: It is very important that the data segment registers (DS, ES,
        // FS, GS, SS) are set to zero. If they are not, the CPU will try to
        // access the GDT to get the segment descriptors for those registers,
        // but the GDT is not set up yet. This will cause a triple fault.
        DS::set_reg(SegmentSelector(0));
        ES::set_reg(SegmentSelector(0));
        FS::set_reg(SegmentSelector(0));
        GS::set_reg(SegmentSelector(0));
        SS::set_reg(SegmentSelector(0));
    }
}
