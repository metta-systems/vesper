gdb-remote 5555
settings set target.require-hardware-breakpoint true
target stop-hook add
bt
disassemble --pc
DONE
