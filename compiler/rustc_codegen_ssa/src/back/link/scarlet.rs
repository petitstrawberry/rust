//! Mark actual Scarlet link outputs for the kernel's native ABI dispatcher.
//!
//! Generic ELF linkers emit ELFOSABI_SYSV even for the custom Scarlet target.
//! This runs only after successful native linking, never on archives or objects.
use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

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
        || !matches!(header[7], 0 | 83)
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
                let mut expected = image(class, machine, kind);
                let mut cursor = Cursor::new(expected.clone());
                mark_header(&mut cursor, class, machine).unwrap();
                expected[7] = 83;
                assert_eq!(cursor.into_inner(), expected);
            }
        }
    }

    #[test]
    fn rejects_wrong_architecture_objects_and_foreign_abi_without_writing() {
        let mut foreign = image(2, 183, 3);
        foreign[7] = 3;
        for original in [image(2, 243, 2), image(2, 183, 1), foreign, vec![0; 8]] {
            let mut cursor = Cursor::new(original.clone());
            assert!(mark_header(&mut cursor, 2, 183).is_err());
            assert_eq!(cursor.into_inner(), original);
        }
    }
}
