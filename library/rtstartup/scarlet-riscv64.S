// Executable-only Scarlet entry. No Rust compiler, libc, TLS, or sysroot is
// required to assemble this object. argc/argv arrive in a0/a1, SP aligned to 16.
.section .text._start,"ax",@progbits
.p2align 2
.weak _start
.type _start,@function
.hidden main
.hidden __init_array_start
.hidden __init_array_end
.option push
.option norelax
_start:
    // envp = argv + argc + 1; auxv follows the envp NULL terminator.
    addi a2, a0, 1
    slli a2, a2, 3
    add a2, a1, a2
    mv a3, a2
1:
    ld t0, 0(a3)
    addi a3, a3, 8
    bnez t0, 1b
    lla a4, main
    lla a5, __init_array_start
    lla a6, __init_array_end
    // std may live in this executable or in one of its Rust dylibs.
    tail __scarlet_start
.size _start, .-_start
.option pop
.section .note.GNU-stack,"",@progbits
