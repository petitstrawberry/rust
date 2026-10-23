# Scarlet rust-analyzer package

`package.py` assembles an already-built native server and the matching Rust
standard-library sources. It uses Python 3.11+ and Git, makes no network requests,
and neither builds nor runs the supplied executable.

## Build on the host

Use the Scarlet-patched **host** Rust toolchain with the selected Scarlet target's
standard library installed. The ordinary upstream toolchain is insufficient.
In Scarlet's development shell, `SCARLET_RUST_TOOLCHAIN` identifies that toolchain;
otherwise set it to its absolute installation path. Run from the `rust-scarlet`
repository root, after the Scarlet dependency/source fixes are present:

```sh
export RUSTC="${SCARLET_RUST_TOOLCHAIN:?}/bin/rustc"
export PATH="$SCARLET_RUST_TOOLCHAIN/bin:$PATH"
ra_target=aarch64-unknown-scarlet
export CARGO_TARGET_DIR="$PWD/target/scarlet-rust-analyzer"
__CARGO_TESTS_ONLY_SRC_ROOT="$PWD/library" RUSTC_BOOTSTRAP=1 \
cargo build --locked --manifest-path src/tools/rust-analyzer/Cargo.toml \
  --release --target "$ra_target" -p rust-analyzer --bin rust-analyzer \
  -Zbuild-std=std,panic_abort -Zbuild-std-features=compiler-builtins-mem
```

The explicit source root rebuilds the native standard library from this checkout,
including `Command::spawn` via `EnvironmentSpawn`. It does not modify the installed
host toolchain or its dependency cache. This requires a Scarlet kernel with the
Environment execution API. Use `riscv64gc-unknown-scarlet` for RV64. Host cross-build success establishes
that an artifact was built. Native startup, Cargo project loading, LSP requests,
and shutdown require separate execution inside Scarlet. This package provides
neither a proc-macro server nor rustfmt, and makes no claim that either works.

## Package the existing executable

Package only after the integration fixes, including the vendored inventory
constructor registration fix, are committed and the supplied binary is rebuilt
from the finalized sources. The script requires license files in HEAD; binary
freshness is the caller's responsibility, and its SHA-256 is recorded.

Initialize `library/backtrace` at the revision pinned by this repository before
packaging. The script requires it to be initialized and at that revision; it
does not update it. Choose the exact version directory of the installed native
Rust toolchain for `ra_sysroot_version`, replacing the example below:

```sh
ra_sysroot_version=vX.Y.Z
cargo vendor --locked --versioned-dirs --manifest-path library/Cargo.toml "$PWD/target/std-vendor"
python3 src/tools/rust-analyzer/scarlet/package.py \
  --binary "$CARGO_TARGET_DIR/$ra_target/release/rust-analyzer" \
  --target "$ra_target" \
  --sysroot-version "$ra_sysroot_version" \
  --vendor-dir "$PWD/target/std-vendor" \
  --output "$PWD/rust-analyzer-$ra_target-$ra_sysroot_version.tar.gz"
```

The output's parent directory must exist. An existing output, including a
symlink, is refused; choose a new name. There is no overwrite option. A temporary
archive beside the destination is removed on failure and atomically published
without replacing a previous file on success.

The standard-library registry dependencies are included for offline Cargo metadata.
The packager verifies their package and file checksums against the committed
`library/Cargo.lock`; their license files remain alongside their sources. A local
Cargo source replacement is installed only within the standard-library source
workspace. User project registries and caches are unaffected.

The archive has paths relative to a Scarlet filesystem root:

```text
bin/rust-analyzer
opt/scarlet/toolchains/rust/<version>/lib/rustlib/src/rust/library/...
share/rust-analyzer/manifest.json
share/licenses/rust-analyzer/LICENSE-{APACHE,MIT}
share/licenses/rust-analyzer/inventory/LICENSE-{APACHE,MIT}
share/licenses/rust-analyzer/salsa/LICENSE-{APACHE,MIT}
share/licenses/rust-analyzer/rust/{COPYRIGHT,LICENSE-APACHE,LICENSE-MIT,...}
```

It can be extracted into a staging root before image creation. The installed
native toolchain and Scarlet runtime must already supply the compiler, Cargo,
and any runtime libraries needed by the executable. If the ELF has an
interpreter, it must be exactly `/bin/scarlet-ld`; that loader belongs to Scarlet
and is not included here. Validation also checks little-endian ELF64, target
machine (AArch64 183 or RISC-V 243), native OSABI `0x53`, executable file mode,
ELF type/segment bounds, and an entry point within an executable load segment.
These structural checks are not a runtime test.

## Source provenance and reproducibility

The script infers the repository root from its own location. It reads committed
Git blobs from the current `HEAD` under `library/` and from the initialized
`library/backtrace` submodule at HEAD's gitlink. Working-tree edits, staged-only
changes, untracked files, and `.git` metadata are not packaged. Directories named
`build`, `target`, `tmp`, `temp`, or `__pycache__` are excluded even if tracked.
Unexpected submodules or non-regular source entries are rejected. Tracked
source license files remain in place; Rust's root license texts, `COPYRIGHT`,
`LICENSES/`, `REUSE.toml`, and `license-metadata.json`, plus rust-analyzer's two
root licenses and the vendored inventory and salsa `LICENSE-APACHE` and `LICENSE-MIT`, are
included in the license directory. The vendor source tree is not modified.

This integration starts from Rust
`26d00a8a3a8489d73f5ae3575880e9f915dfd0f2`, used by the published native
release `79f356`, and includes subsequent Scarlet process-spawn fixes. The `--sysroot-version` argument is the actual
installed release directory name, not either abbreviated commit. The script
records the HEAD it reads; it does not hardcode that revision or infer which
native release is installed.

The manifest records Git HEAD, the backtrace commit, binary SHA-256 and ELF
identity, target, toolchain version (the supplied `--sysroot-version`), source
file count, and source tree SHA-256. The tree hash covers a list sorted by path
relative to the installed `library/` directory. Each record has `path`, `mode`
(four octal digits), and the file-content `sha256`. Hash the UTF-8 encoding of
Python's `json.dumps(records, sort_keys=True, separators=(",", ":"))`, with its
default ASCII escaping, to reproduce it. Source HEAD describes the packaged
sources; the supplied binary's hash identifies it independently and does not
prove that it was built from a clean HEAD.

Entry order is fixed, tar ownership is root/0, all timestamps are zero, modes
are fixed (or taken from tracked source modes), and gzip has no embedded filename.
Identical inputs with the same Python/zlib implementation produce identical
archive bytes. The manifest always records `runtime_verified: false`; native
acceptance evidence is maintained separately.

For a packaging-script-only check, without building or packaging a binary:

```sh
python3 -m py_compile src/tools/rust-analyzer/scarlet/package.py
python3 src/tools/rust-analyzer/scarlet/package.py --help
```
