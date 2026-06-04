# Scarlet Native

Scarlet Native targets are maintained in Scarlet's Rust fork and are not
intended for upstream Rust distribution.

Target triples:

```text
riscv64gc-unknown-scarlet
aarch64-unknown-scarlet
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
