# Scarlet Native

Scarlet Native targets are maintained in Scarlet's Rust fork and are not
intended for upstream Rust distribution.

Target triples:

```text
riscv32gc-unknown-scarlet
riscv64gc-unknown-scarlet
riscv64a23-unknown-scarlet
aarch64-unknown-scarlet
```

The RISC-V variants follow the ISA and calling conventions of the corresponding
Linux targets in this fork:

| Target | ISA | ABI | Pointer width | Maximum atomic width |
| --- | --- | --- | --- | --- |
| `riscv32gc-unknown-scarlet` | RV32GC | `ilp32d` | 32 | 32 |
| `riscv64gc-unknown-scarlet` | RV64GC | `lp64d` | 64 | 64 |
| `riscv64a23-unknown-scarlet` | RVA23 (`+rva23u64`) | `lp64d` | 64 | 64 |

GC includes IMAFDC, Zicsr and Zifencei. RVA23 requires a processor implementing
that profile and Scarlet support for its userspace architectural state,
including vector registers; use the GC target for older processors.
User programs may use hardware single- and double-precision floating point.
All linked code must use the same ABI; rebuild libraries previously built
with a soft-float `ilp32` or `lp64` ABI. All Scarlet targets link statically
with `rust-lld`, abort on panic, and use OS-level thread-local storage.

Build the compiler and standard library from this fork:

```sh
./x build --stage 1 --warnings warn --target riscv32gc-unknown-scarlet library/std
./x build --stage 1 --warnings warn --target riscv64gc-unknown-scarlet,riscv64a23-unknown-scarlet library/std
```

The RV32 standard library requires Scarlet's Native wide-scalar syscall ABI:
64-bit time values and file offsets use consecutive low/high argument words
without register-pair padding, and wide results are returned in `a0`/`a1`.

`scarlet-smoke/riscv-std.rs` exercises floating-point calls, thread-local storage
and destructors, a five-second sleep, and file seeks beyond 4 GiB. Build it with
the stage1 compiler and run the resulting executable on Scarlet:

```sh
build/<host>/stage1/bin/rustc --target riscv32gc-unknown-scarlet \
    scarlet-smoke/riscv-std.rs -o /tmp/scarlet-riscv-std
```

The initial `std` bring-up supports aborting panics, anonymous memory mappings
for the global allocator, `std::process::exit`, and standard input/output/error
through Scarlet stream handles.

```rust
fn main() {
    let mut name = String::new();
    std::io::stdin().read_line(&mut name).unwrap();
    println!("Hello from real std on Scarlet!");
    eprintln!("stderr works too");
    std::process::exit(0);
}
```
