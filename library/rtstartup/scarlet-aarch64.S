// Executable-only Scarlet entry. No Rust compiler, libc, TLS, or sysroot is
// required to assemble this object. argc/argv arrive in x0/x1, SP aligned to 16.
.section .text._start,"ax",%progbits
.p2align 2
.weak _start
.type _start,%function
.hidden main
.hidden __init_array_start
.hidden __init_array_end
_start:
    // envp = argv + argc + 1; auxv follows the envp NULL terminator.
    add x2, x0, #1
    add x2, x1, x2, lsl #3
    mov x3, x2
1:
    ldr x7, [x3], #8
    cbnz x7, 1b
    adrp x4, main
    add x4, x4, :lo12:main
    adrp x5, __init_array_start
    add x5, x5, :lo12:__init_array_start
    adrp x6, __init_array_end
    add x6, x6, :lo12:__init_array_end
    // std may live in this executable or in one of its Rust dylibs.
    b __scarlet_start
.size _start, .-_start
.section .note.GNU-stack,"",%progbits
