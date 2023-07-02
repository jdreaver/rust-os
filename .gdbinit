# Pretty printing
set print pretty on

# My layout
tui new-layout rust-os {-horizontal src 1 {asm 1 regs 1} 1} 2 status 0 cmd 1
tui layout rust-os

# Log output to gdb.txt so we don't need to scroll in TUI mode
set logging file gdb.log
set logging enabled on
set trace-commands on

# Helper functions

# I used this during an investigation into a General Protection Fault caused by
# the ret instruction in the task switching switch_to_task assembly. I wanted to
# see the stack (specifically just the instruction pointer we are returning to)
# and registers at the point of the fault. This command will dump the registers
# and stack, then continue execution.
define dump_continue
  info reg rax rbx rcx rdx rsi rdi rbp rsp r8 r9 r10 r11 r12 r13 r14 r15 rip eflags cs ss ds es fs gs gs_base k_gs_base cr3
  info stack
  x /1xg $rsp
  continue
end
