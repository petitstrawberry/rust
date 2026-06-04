use crate::ffi::CStr;
use crate::io;
use crate::num::NonZero;
use crate::sys::pal::abi;
use crate::thread::ThreadInit;
use crate::time::Duration;

// Silence dead code warnings for the otherwise unused ThreadInit::init() call
// until Scarlet wires std thread creation to Native task/thread handles.
#[expect(dead_code)]
fn dummy_init_call(init: Box<ThreadInit>) {
    drop(init.init());
}

pub struct Thread(!);

pub const DEFAULT_MIN_STACK_SIZE: usize = 64 * 1024;

impl Thread {
    // unsafe: see thread::Builder::spawn_unchecked for safety requirements
    pub unsafe fn new(_stack: usize, _init: Box<ThreadInit>) -> io::Result<Thread> {
        // TODO(scarlet): wire this to the Native thread creation ABI once the
        // std thread contract is mapped to Scarlet task/thread handles.
        Err(io::Error::UNSUPPORTED_PLATFORM)
    }

    pub fn join(self) {
        self.0
    }
}

pub fn available_parallelism() -> io::Result<NonZero<usize>> {
    // TODO(scarlet): expose the online CPU count from the kernel.
    Ok(NonZero::<usize>::MIN)
}

pub fn current_os_id() -> Option<u64> {
    // TODO(scarlet): expose a Native thread id distinct from the process id.
    None
}

pub fn yield_now() {
    let _ = abi::thread_yield();
}

#[allow(dead_code)]
pub fn set_name(_name: &CStr) {
    // TODO(scarlet): expose a Native thread naming operation.
}

pub fn sleep(dur: Duration) {
    let nanos = dur.as_nanos().min(usize::MAX as u128) as u64;
    let _ = abi::sleep(nanos);
}
