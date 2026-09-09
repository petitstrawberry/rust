//! Run on Scarlet to exercise floating point, TLS, threads and wide Native scalars.

use std::cell::Cell;
use std::hint::black_box;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use std::{fs, thread};

thread_local! {
    static VALUE: Cell<usize> = const { Cell::new(0) };
    static CLEANUP: Cleanup = const { Cleanup };
}

static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Cleanup;

impl Drop for Cleanup {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

#[inline(never)]
extern "C" fn add_f32(a: f32, b: f32) -> f32 {
    a + b
}

#[inline(never)]
extern "C" fn add_f64(a: f64, b: f64) -> f64 {
    a + b
}

fn main() -> io::Result<()> {
    assert_eq!(add_f32(black_box(1.25), black_box(2.5)), 3.75);
    assert_eq!(add_f64(black_box(1.25), black_box(2.5)), 3.75);
    VALUE.set(42);
    let child = thread::spawn(|| {
        assert_eq!(VALUE.get(), 0);
        VALUE.set(7);
        CLEANUP.with(|_| ());
        thread::yield_now();
        assert_eq!(VALUE.get(), 7);
        add_f64(black_box(0.5), black_box(0.25))
    });
    assert_eq!(child.join().unwrap(), 0.75);
    assert_eq!(VALUE.get(), 42);
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);

    // Cross the RV32 nanosecond boundary without truncating sleep or clock results.
    let start = Instant::now();
    thread::sleep(Duration::from_secs(5));
    assert!(start.elapsed() >= Duration::from_secs(5));
    let path = format!("/tmp/scarlet-riscv-std-{}", std::process::id());
    let mut file = fs::File::options().read(true).write(true).create_new(true).open(&path)?;
    file.write_all(b"wide ABI")?;
    assert_eq!(file.seek(SeekFrom::Current(-3))?, 5);
    let mut tail = String::new();
    file.read_to_string(&mut tail)?;
    assert_eq!(tail, "ABI");
    // Seeking beyond EOF tests the high word without allocating a multi-GB file.
    let wide_position = (1u64 << 32) + 123;
    assert_eq!(file.seek(SeekFrom::Start(wide_position))?, wide_position);
    assert_eq!(file.seek(SeekFrom::Current(-7))?, wide_position - 7);
    file.set_len(4)?;
    assert_eq!(file.metadata()?.len(), 4);
    drop(file);
    fs::remove_file(path)?;
    println!("scarlet-riscv-std: ok");
    Ok(())
}
