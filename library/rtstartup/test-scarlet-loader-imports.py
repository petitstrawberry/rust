#!/usr/bin/env python3
"""Check native loader imports with a built toolchain; never build a compiler."""
import argparse
from pathlib import Path
import re
import struct
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rustc", required=True)
    parser.add_argument("--readelf", default="llvm-readelf")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="scarlet-loader-imports-") as temporary:
        root = Path(temporary)
        source = root / "imports.rs"
        source.write_text('''
unsafe extern "C" {
    fn dlopen(path: *const u8, flags: i32) -> *mut u8;
    fn dlsym(handle: *mut u8, name: *const u8) -> *mut u8;
    fn dlclose(handle: *mut u8) -> i32;
    fn dlerror() -> *const u8;
    static environ: *const *const u8;
}
fn main() {
    unsafe {
        let handle = dlopen(std::ptr::null(), 2);
        println!("{:p} {:p} {} {:p}", dlsym(handle, c"probe".as_ptr().cast()),
                 dlerror(), dlclose(handle), environ);
        std::hint::black_box(dlopen as *const ());
    }
}
''')
        missing = root / "missing.rs"
        missing.write_text('''
unsafe extern "C" {
    fn missing_application_function();
    fn dlopen(path: *const u8, flags: i32) -> *mut u8;
}
fn main() {
    unsafe {
        std::hint::black_box(dlopen(std::ptr::null(), 2));
        missing_application_function();
    }
}
''')
        hello = root / "hello.rs"
        hello.write_text('fn main() { println!("native hello"); }\n')
        for target, machine in [("aarch64-unknown-scarlet", 183),
                                ("riscv64gc-unknown-scarlet", 243)]:
            command = [args.rustc, "--edition=2024", "--target", target,
                       "-C", "panic=abort", "-C", "opt-level=2", "-D", "linker-messages"]
            binary = root / target
            subprocess.run([*command, str(source), "-o", str(binary)], check=True)
            data = binary.read_bytes()
            assert data[:8] == b"\x7fELF\x02\x01\x01\x53"
            assert struct.unpack_from("<HH", data, 16) == (3, machine)
            output = subprocess.check_output(
                [args.readelf, "-W", "-l", "-d", "--dyn-syms", "-r", str(binary)], text=True)
            assert "[Requesting program interpreter: /bin/scarlet-ld]" in output, output
            assert "(NEEDED)" not in output, output
            assert "(RUNPATH)" not in output and "(RPATH)" not in output
            imports = {line.split()[-1] for line in output.splitlines()
                       if re.search(r"\bGLOBAL\s+DEFAULT\s+UND\b", line)}
            expected = {"dlopen", "dlsym", "dlclose", "dlerror", "environ"}
            assert imports == expected, imports
            for symbol in expected:
                assert any(re.search(r"\bR_[A-Z0-9_]+\b", line)
                           and re.search(r"\b" + symbol + r"\s*\+", line)
                           for line in output.splitlines()), symbol
            subprocess.run([*command, str(hello), "-o", str(root / "hello")], check=True)
            bad = root / "must-not-exist"
            result = subprocess.run([*command, str(missing), "-o", str(bad)],
                                    capture_output=True, text=True)
            assert result.returncode != 0 and "missing_application_function" in result.stderr
            assert not bad.exists(), "failed link left an executable behind"
            print(target + ": loader imports/relocations, ordinary executable, unknown-symbol rejection passed")


if __name__ == "__main__":
    main()
