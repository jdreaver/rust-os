# Pretty printing
set print pretty on

# My layout
tui new-layout rust-os {-horizontal src 1 {asm 1 regs 1} 1} 2 status 0 cmd 1
tui layout rust-os

# Log output to gdb.txt so we don't need to scroll in TUI mode
set logging file gdb.log
set logging enabled on
set trace-commands on
