#!/usr/bin/env python3
"""Assemble/link Scarlet CRT contracts with stock LLVM; no Rust build needed."""
import argparse
from pathlib import Path
import subprocess
import tempfile


def run(*args):
    subprocess.run([str(arg) for arg in args], check=True)


def symbols(readelf, path):
    output = subprocess.check_output([readelf, "-Ws", str(path)], text=True)
    return [line.split() for line in output.splitlines() if " UND " in line or " FUNC " in line]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cc", default="clang")
    parser.add_argument("--linker", default="ld.lld")
    parser.add_argument("--readelf", default="llvm-readelf")
    args = parser.parse_args()
    source = Path(__file__).resolve().parent
    with tempfile.TemporaryDirectory(prefix="scarlet-crt-") as temporary:
        out = Path(temporary)
        (out / "main.c").write_text("""
static volatile int initialized;
__attribute__((constructor)) static void initialize(void) { initialized = 1; }
int main(int argc, const char **argv) { return initialized + argc + (argv != 0); }
""")
        (out / "empty.c").write_text("int main(int argc, const char **argv) { return argc; }\n")
        # This substitutes for std to check the executable/DSO linking boundary;
        # it does not claim to test the Rust runtime or execute Scarlet code.
        (out / "runtime.c").write_text("""
typedef __INTPTR_TYPE__ word;
typedef void (*ctor)(void);
__attribute__((noreturn)) void __scarlet_start(word argc, const char **argv,
    const char **envp, const word *auxv, int (*main_fn)(int, const char **),
    ctor *begin, ctor *end) {
    while (begin < end) (*begin++)();
    volatile int result = main_fn((int)argc, argv) + (envp != 0) + (auxv != 0);
    (void)result;
    for (;;) {}
}
""")
        for arch in ("aarch64", "riscv64"):
            flags = [f"--target={arch}-unknown-none"]
            if arch == "riscv64":
                flags += ["-march=rv64gc", "-mabi=lp64d"]
            custom = out / f"{arch}-custom.S"
            directive = "%progbits" if arch == "aarch64" else "@progbits"
            jump = "b" if arch == "aarch64" else "j"
            custom.write_text(f"""
.section .text.custom,"ax",{directive}
.global _start
.global custom_entry
_start:
custom_entry:
    {jump} custom_entry
""")
            objects = {}
            for name, input_path in (
                ("crt", source / f"scarlet-{arch}.S"),
                ("main", out / "main.c"),
                ("empty", out / "empty.c"),
                ("runtime", out / "runtime.c"),
                ("custom", custom),
            ):
                objects[name] = out / f"{arch}-{name}.o"
                run(args.cc, *flags, "-fPIC", "-ffunction-sections", "-fno-stack-protector",
                    "-c", input_path, "-o", objects[name])

            def link(name, *inputs):
                path = out / f"{arch}-{name}"
                run(args.linker, "--gc-sections", "--no-undefined", "-z", "max-page-size=4096",
                    "-o", path, *inputs)
                return path

            crt, executable, runtime, raw = (objects[key] for key in ("crt", "main", "runtime", "custom"))
            link("static.elf", crt, executable, runtime)
            link("no-constructors.elf", crt, objects["empty"], runtime)
            dso = link("runtime.so", "-shared", "-soname", f"{arch}-runtime.so", runtime)
            dynamic = link("dynamic.elf", crt, executable, dso, "--dynamic-linker=/bin/scarlet-ld")
            link("pie.elf", "-pie", crt, executable, dso, "--dynamic-linker=/bin/scarlet-ld")
            standalone = link("no-std.elf", crt, raw)
            alternate = link("custom-entry.elf", "-e", "custom_entry", crt, raw)
            forbidden = {"main", "__scarlet_start", "__init_array_start", "__init_array_end"}
            for binary in (dso, standalone, alternate):
                entries = symbols(args.readelf, binary)
                assert not any("UND" in fields and fields[-1] in forbidden for fields in entries), binary
            assert not any(fields[-1] == "_start" for fields in symbols(args.readelf, dso)), dso
            entries = symbols(args.readelf, dynamic)
            assert any(fields[-1] == "_start" and "UND" not in fields for fields in entries), dynamic
            assert any(fields[-1] == "__scarlet_start" and "UND" in fields for fields in entries), dynamic
            print(f"{arch}: static, dynamic, PIE, empty constructors, shared runtime, no_std/custom entry passed")


if __name__ == "__main__":
    main()
