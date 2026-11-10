//! Mark actual Scarlet link outputs for the kernel's native ABI dispatcher.
//!
//! Generic ELF linkers emit ELFOSABI_SYSV or ELFOSABI_GNU even for the custom
//! Scarlet target. LLVM uses the GNU tag for extensions such as SHF_GNU_RETAIN.
//! This runs only after successful native linking, never on archives or objects.
use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

// These symbols are supplied by the resident /bin/scarlet-ld, without a
// DT_NEEDED library. LLD otherwise either rejects them or resolves them to
// zero in a PIE with no startup DSOs. Keep them preemptible and inspect every
// unresolved-symbol diagnostic before accepting the output. In particular,
// --unresolved-symbols=ignore-all is not an acceptable platform default.
pub(super) const LOADER_LINK_ARGS: &[&str] =
    &["--warn-unresolved-symbols", "-z", "dynamic-undefined-weak", "-z", "now"];

const LOADER_SYMBOLS: &[&str] = &["dlopen", "dlsym", "dlclose", "dlerror", "environ"];

/// Remove diagnostics for interpreter-provided imports, rejecting all other
/// unresolved symbols. Input is from LLD with localization disabled by rustc.
pub(super) fn filter_loader_diagnostics(output: &[u8]) -> Result<Vec<u8>, String> {
    let mut filtered = Vec::with_capacity(output.len());
    let mut loader_reference = false;
    for line in output.split_inclusive(|&byte| byte == b'\n') {
        let text = String::from_utf8_lossy(line);
        if let Some((_, name)) = text.split_once(": warning: undefined symbol: ") {
            if !LOADER_SYMBOLS.contains(&name.trim()) {
                return Err(format!("unresolved Scarlet symbol: {}", name.trim()));
            }
            loader_reference = true;
        } else if text.contains(": warning: undefined ") {
            // Hidden, local, versioned and unrecognized diagnostic forms must
            // never become accepted interpreter imports.
            return Err(format!("unsupported unresolved Scarlet symbol: {}", text.trim()));
        } else if loader_reference && (text.starts_with(">>>") || text.trim().is_empty()) {
            continue;
        } else {
            loader_reference = false;
            filtered.extend_from_slice(line);
        }
    }
    Ok(filtered)
}

pub(super) fn mark_output(path: &Path, class: u8, machine: u16) -> io::Result<()> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    mark_header(&mut file, class, machine)
}

fn mark_header(file: &mut (impl Read + Seek + Write), class: u8, machine: u16) -> io::Result<()> {
    let mut header = [0u8; 20];
    file.read_exact(&mut header)?;
    if header[..4] != *b"\x7fELF"
        || header[4] != class
        || header[5] != 1
        || header[6] != 1
        || !matches!(header[7], 0 | 3 | 83)
        || !matches!(u16::from_le_bytes([header[16], header[17]]), 2 | 3)
        || u16::from_le_bytes([header[18], header[19]]) != machine
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "linker produced an unexpected Scarlet ELF header",
        ));
    }
    file.seek(SeekFrom::Start(7))?;
    file.write_all(&[83])
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn image(class: u8, machine: u16, kind: u16) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..7].copy_from_slice(&[0x7f, b'E', b'L', b'F', class, 1, 1]);
        bytes[16..18].copy_from_slice(&kind.to_le_bytes());
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes[100] = 42;
        bytes
    }

    #[test]
    fn marks_only_osabi_for_native_executables_and_dsos() {
        for (class, machine) in [(2, 183), (2, 243), (1, 243)] {
            for kind in [2, 3] {
                for osabi in [0, 3, 83] {
                    let mut expected = image(class, machine, kind);
                    expected[7] = osabi;
                    let mut cursor = Cursor::new(expected.clone());
                    mark_header(&mut cursor, class, machine).unwrap();
                    expected[7] = 83;
                    assert_eq!(cursor.into_inner(), expected);
                }
            }
        }
    }

    #[test]
    fn rejects_wrong_architecture_objects_and_foreign_abi_without_writing() {
        let mut foreign = image(2, 183, 3);
        foreign[7] = 9; // ELFOSABI_FREEBSD remains incompatible.
        for original in [image(2, 243, 2), image(2, 183, 1), foreign, vec![0; 8]] {
            let mut cursor = Cursor::new(original.clone());
            assert!(mark_header(&mut cursor, 2, 183).is_err());
            assert_eq!(cursor.into_inner(), original);
        }
    }

    #[test]
    fn accepts_only_resident_loader_imports_and_preserves_other_diagnostics() {
        let mut output = String::new();
        for symbol in LOADER_SYMBOLS {
            output.push_str(&format!(
                "rust-lld: warning: undefined symbol: {symbol}\n>>> referenced by input.o\n\n"
            ));
        }
        output.push_str("rust-lld: warning: unrelated warning\n>>> its reference\n");
        assert_eq!(
            filter_loader_diagnostics(output.as_bytes()).unwrap(),
            b"rust-lld: warning: unrelated warning\n>>> its reference\n"
        );
    }

    #[test]
    fn unknown_and_hidden_symbols_remain_errors() {
        for diagnostic in [
            "rust-lld: warning: undefined symbol: missing_application_function\n",
            "rust-lld: warning: undefined hidden symbol: dlopen\n",
            "rust-lld: warning: undefined symbol: dlopen@VERSION\n",
            "rust-lld: warning: undefined symbol: dlopen\n>>> reference\n\
             rust-lld: warning: undefined symbol: missing_application_function\n",
        ] {
            assert!(filter_loader_diagnostics(diagnostic.as_bytes()).is_err());
        }
    }
}
