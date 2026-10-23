#!/usr/bin/env python3
"""Package an already-built Scarlet rust-analyzer and its pinned Rust sources."""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import struct
import subprocess
import tarfile
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[4]
TARGETS = {"aarch64-unknown-scarlet": 183, "riscv64gc-unknown-scarlet": 243}
INTERPRETER = "/bin/scarlet-ld"
MANIFEST = "share/rust-analyzer/manifest.json"
LICENSE_PREFIX = "share/licenses/rust-analyzer"
RUST_LICENSES = (
    "COPYRIGHT", "LICENSE-APACHE", "LICENSE-MIT", "LICENSES",
    "REUSE.toml", "license-metadata.json",
)
RA_LICENSES = (
    "src/tools/rust-analyzer/LICENSE-APACHE",
    "src/tools/rust-analyzer/LICENSE-MIT",
)
INVENTORY_LICENSES = (
    "src/tools/rust-analyzer/scarlet/vendor/inventory/LICENSE-APACHE",
    "src/tools/rust-analyzer/scarlet/vendor/inventory/LICENSE-MIT",
)
SALSA_LICENSES = (
    "src/tools/rust-analyzer/scarlet/vendor/salsa/LICENSE-APACHE",
    "src/tools/rust-analyzer/scarlet/vendor/salsa/LICENSE-MIT",
)


def git(repo, *args):
    return subprocess.check_output(["git", "-C", str(repo), *args])


def tree(repo, commit, *paths):
    entries = []
    for record in git(repo, "ls-tree", "-rz", "--full-tree", commit, "--", *paths).split(b"\0"):
        if record:
            metadata, name = record.split(b"\t", 1)
            mode, kind, oid = metadata.decode("ascii").split()
            entries.append((name.decode("utf-8"), mode, kind, oid))
    return sorted(entries)


def source_files(entries):
    result = []
    for name, mode, kind, oid in entries:
        parts = PurePosixPath(name).parts
        if ".git" in parts or any(
            part in {"build", "target", "tmp", "temp", "__pycache__"} for part in parts[:-1]
        ):
            continue
        if kind != "blob" or mode not in {"100644", "100755"}:
            raise ValueError(f"unsupported tracked source entry: {mode} {name}")
        result.append((name, mode, kind, oid))
    return result


def blobs(repo, entries):
    # Read Git objects, never working-tree files or export-ignore-filtered archives.
    with subprocess.Popen(
        ["git", "-C", str(repo), "cat-file", "--batch"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    ) as process:
        try:
            for name, mode, _, oid in entries:
                process.stdin.write(oid.encode("ascii") + b"\n")
                process.stdin.flush()
                header = process.stdout.readline().split()
                if len(header) != 3 or header[:2] != [oid.encode("ascii"), b"blob"]:
                    raise ValueError(f"cannot read tracked blob: {name}")
                size = int(header[2])
                content = process.stdout.read(size)
                if len(content) != size or process.stdout.read(1) != b"\n":
                    raise ValueError(f"truncated tracked blob: {name}")
                yield name, int(mode, 8) & 0o777, content
            process.stdin.close()
            if process.wait() != 0:
                raise ValueError("git cat-file failed")
        finally:
            if process.poll() is None:
                process.kill()


def elf(binary, machine):
    if len(binary) < 64 or binary[:8] != b"\x7fELF\x02\x01\x01\x53":
        raise ValueError("binary must be little-endian ELF64 with Scarlet OSABI 0x53")
    elf_type, actual_machine, version = struct.unpack_from("<HHI", binary, 16)
    entry, phoff = struct.unpack_from("<QQ", binary, 24)
    ehsize, phentsize, phnum = struct.unpack_from("<HHH", binary, 52)
    if actual_machine != machine or elf_type not in (2, 3) or version != 1:
        raise ValueError("binary has the wrong target machine or is not an ELF executable")
    if (
        ehsize != 64 or phentsize != 56 or not 0 < phnum < 65535
        or phoff < 64 or phoff + phentsize * phnum > len(binary)
    ):
        raise ValueError("invalid ELF program header table")
    interpreter = None
    executable_entry = False
    for index in range(phnum):
        kind, flags, offset, address, _, filesz, memsz, _ = struct.unpack_from(
            "<IIQQQQQQ", binary, phoff + index * phentsize
        )
        if offset + filesz > len(binary) or (kind == 1 and filesz > memsz):
            raise ValueError("invalid ELF segment bounds")
        if kind == 1 and flags & 1 and entry != 0 and address <= entry < address + memsz:
            executable_entry = True
        if kind == 3:
            if interpreter is not None or binary[offset:offset + filesz] != INTERPRETER.encode() + b"\0":
                raise ValueError(f"ELF interpreter must be exactly {INTERPRETER}")
            interpreter = INTERPRETER
    if not executable_entry:
        raise ValueError("ELF entry point is not inside an executable PT_LOAD segment")
    return {"machine": actual_machine, "osabi": 0x53, "type": elf_type, "interpreter": interpreter}


def add_file(archive, name, mode, content):
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts:
        raise ValueError(f"unsafe archive path: {name}")
    info = tarfile.TarInfo(name)
    info.size = len(content)
    info.mode = mode
    info.mtime = 0
    info.uid = info.gid = 0
    info.uname = info.gname = "root"
    archive.addfile(info, io.BytesIO(content))


def vendor_files(directory, head):
    """Require the complete, checksum-verified registry closure of Rust sources."""
    lock = tomllib.loads(git(ROOT, "show", f"{head}:library/Cargo.lock").decode())
    packages = [p for p in lock["package"] if p.get("source", "").startswith("registry+")]
    for package in sorted(packages, key=lambda p: (p["name"], p["version"])):
        name = f"{package['name']}-{package['version']}"
        root = directory / name
        checksum_path = root / ".cargo-checksum.json"
        checksums = json.loads(checksum_path.read_text())
        if checksums["package"] != package["checksum"]:
            raise ValueError(f"vendored package does not match library/Cargo.lock: {name}")
        expected = set(checksums["files"]) | {".cargo-checksum.json"}
        actual = {p.relative_to(root).as_posix() for p in root.rglob("*") if not p.is_dir()}
        if expected != actual:
            raise ValueError(f"missing or extra vendored files: {name}")
        for relative in sorted(expected):
            path = root / relative
            if path.is_symlink() or not path.is_file() or ".." in Path(relative).parts:
                raise ValueError(f"unsupported vendor entry: {path}")
            content = path.read_bytes()
            if relative != ".cargo-checksum.json" and hashlib.sha256(content).hexdigest() != checksums["files"][relative]:
                raise ValueError(f"vendor checksum mismatch: {path}")
            yield f"vendor/{name}/{relative}", (0o755 if path.stat().st_mode & 0o111 else 0o644), content
    config = b'[source.crates-io]\nreplace-with = "scarlet-std-vendor"\n[source.scarlet-std-vendor]\ndirectory = "vendor"\n'
    yield ".cargo/config.toml", 0o644, config


def package(args):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]*", args.sysroot_version):
        raise ValueError("--sysroot-version must be a single version directory name")
    output = args.output.absolute()
    if os.path.lexists(output):
        raise ValueError(f"output already exists; choose a new --output path: {output}")
    mode = args.binary.lstat().st_mode
    if not stat.S_ISREG(mode) or not mode & 0o111:
        raise ValueError("--binary must be a regular file with an executable mode bit")
    binary = args.binary.read_bytes()
    identity = elf(binary, TARGETS[args.target])
    head = git(ROOT, "rev-parse", "HEAD").decode().strip()
    sources = tree(ROOT, head, "library")
    backtrace = ROOT / "library/backtrace"
    gitlinks = [item for item in sources if item[2] == "commit"]
    if len(gitlinks) != 1 or gitlinks[0][:3] != ("library/backtrace", "160000", "commit"):
        raise ValueError("expected exactly the library/backtrace source submodule")
    backtrace_head = gitlinks[0][3]
    if not (backtrace / ".git").exists() or Path(
        git(backtrace, "rev-parse", "--show-toplevel").decode().strip()
    ).resolve() != backtrace.resolve():
        raise ValueError("library/backtrace must already be initialized")
    if git(backtrace, "rev-parse", "HEAD").decode().strip() != backtrace_head:
        raise ValueError("library/backtrace HEAD does not match the superproject gitlink")
    sources = source_files([item for item in sources if item not in gitlinks])
    backtrace_sources = source_files(tree(backtrace, backtrace_head))
    licenses = tree(ROOT, head, *RUST_LICENSES, *RA_LICENSES, *INVENTORY_LICENSES, *SALSA_LICENSES)
    required = (set(RUST_LICENSES) - {"LICENSES"}) | set(RA_LICENSES) | set(INVENTORY_LICENSES) | set(SALSA_LICENSES)
    if not required.issubset({item[0] for item in licenses}):
        raise ValueError("required Rust, rust-analyzer, inventory, or salsa licenses are missing from HEAD")
    licenses = source_files(licenses)
    source_prefix = f"opt/scarlet/toolchains/rust/{args.sysroot_version}/lib/rustlib/src/rust/library"
    manifest = {
        "schema": 1,
        "kind": "scarlet-rust-analyzer",
        "git_head": head,
        "target": args.target,
        "toolchain_version": args.sysroot_version,
        "binary": {"path": "/bin/rust-analyzer", "sha256": hashlib.sha256(binary).hexdigest(), **identity},
        "sources": {"path": "/" + source_prefix, "backtrace_git_head": backtrace_head},
        "runtime_verified": False,
    }
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(prefix=f".{output.name}.", dir=output.parent, delete=False) as stream:
            temporary = Path(stream.name)
            with gzip.GzipFile(filename="", fileobj=stream, mode="wb", compresslevel=9, mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
                    add_file(archive, "bin/rust-analyzer", 0o755, binary)
                    inventory = []
                    for repo, entries, strip, prefix in (
                        (ROOT, sources, "library/", ""),
                        (backtrace, backtrace_sources, "", "backtrace/"),
                    ):
                        for name, mode, content in blobs(repo, entries):
                            relative = prefix + name.removeprefix(strip)
                            add_file(archive, f"{source_prefix}/{relative}", mode, content)
                            inventory.append({
                                "path": relative, "mode": f"{mode:04o}",
                                "sha256": hashlib.sha256(content).hexdigest(),
                            })
                    for relative, mode, content in vendor_files(args.vendor_dir, head):
                        add_file(archive, f"{source_prefix}/{relative}", mode, content)
                        inventory.append({
                            "path": relative, "mode": f"{mode:04o}",
                            "sha256": hashlib.sha256(content).hexdigest(),
                        })
                    inventory.sort(key=lambda item: item["path"])
                    encoded = json.dumps(inventory, sort_keys=True, separators=(",", ":")).encode()
                    manifest["sources"].update({
                        "tree_sha256": hashlib.sha256(encoded).hexdigest(),
                        "file_count": len(inventory),
                        "hash_format": "sha256-canonical-json-path-mode-sha256-v1",
                    })
                    for name, mode, content in blobs(ROOT, licenses):
                        if name in RA_LICENSES:
                            relative = Path(name).name
                        elif name in INVENTORY_LICENSES:
                            relative = "inventory/" + Path(name).name
                        elif name in SALSA_LICENSES:
                            relative = "salsa/" + Path(name).name
                        else:
                            relative = "rust/" + name
                        add_file(archive, f"{LICENSE_PREFIX}/{relative}", 0o644, content)
                    add_file(archive, MANIFEST, 0o644, (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode())
            stream.flush()
            os.fsync(stream.fileno())
            os.fchmod(stream.fileno(), 0o644)
        # Same-directory hard-link publication is atomic and refuses even a
        # dangling symlink created at the output path while packaging.
        os.link(temporary, output)
    finally:
        if temporary is not None:
            temporary.unlink()
    print(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="already-built native rust-analyzer ELF")
    parser.add_argument("--target", choices=sorted(TARGETS), required=True)
    parser.add_argument("--sysroot-version", required=True, help="installed native Rust toolchain version directory")
    parser.add_argument("--vendor-dir", type=Path, required=True, help="cargo vendor --locked --versioned-dirs output for library/Cargo.toml")
    parser.add_argument("--output", type=Path, required=True, help="new .tar.gz path; parent directory must exist")
    args = parser.parse_args()
    try:
        package(args)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"error: {error}\n")


if __name__ == "__main__":
    main()
