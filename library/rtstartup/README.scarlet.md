# Scarlet executable startup

`scarlet-crt0.o` supplies a weak `_start` in its own text section. The Scarlet
AArch64 and RV64 targets add this object only to executable link output kinds;
Rust dylibs, cdylibs, and proc macros do not receive it. It calls the C ABI
`__scarlet_start` exported by std, which can live in the executable or a Rust
dylib containing statically linked std.

The entry preserves the kernel's stack alignment and forwards these seven
native register arguments:

1. `argc` (`x0` / `a0`)
2. `argv` (`x1` / `a1`)
3. `envp`, immediately after the `argv` NULL
4. `auxv`, immediately after the `envp` NULL
5. the executable's `main` function
6. the executable's `__init_array_start`
7. the executable's `__init_array_end`

Std publishes auxv and initializes the environment before calling the main
image's constructors. The userspace loader initializes dependency constructors.
Std contains no direct reference to `main` or linker-defined constructor bounds.

Bootstrap assembles the architecture source with the configured target C
compiler, then installs the object in the target sysroot's `lib` directory. For
Clang it selects `aarch64-unknown-none` or `riscv64-unknown-none` explicitly; this
requires neither Scarlet support in the stage0 Rust compiler nor libc headers.
RV64 uses the `rv64gc` / `lp64d` base ISA and ABI, including for the RVA23
profile target. RV32 retains its existing static Rust entry.

A `no_std` application supplying a strong `_start` overrides the weak CRT entry.
Rust's normal section garbage collection discards the unused CRT and its runtime
references. An application selecting another entry with `-e` also discards it
unless that entry explicitly calls `_start`. Custom targets that disable section
garbage collection and provide their own startup should clear `pre-link-objects`.

Run the lightweight assembly/link checks with stock LLVM tools:

```sh
python3 library/rtstartup/test-scarlet-crt.py \
  --cc clang --linker ld.lld --readelf llvm-readelf
```

These checks cover static, dynamic and PIE executables, empty constructor arrays,
a shared runtime without executable-only symbols, and standalone custom entries.
They substitute a tiny C runtime and do not replace building std or booting a
native Rust executable.
