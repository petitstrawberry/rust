//! Read both stdout and stderr of child without deadlocks.
//!
//! <https://github.com/rust-lang/cargo/blob/905af549966f23a9288e9993a85d1249a5436556/crates/cargo-util/src/read2.rs>
//! <https://github.com/rust-lang/cargo/blob/58a961314437258065e23cb6316dfc121d96fb71/crates/cargo-util/src/process_builder.rs#L231>

use std::{
    io,
    process::{ChildStderr, ChildStdout, Command, Output, Stdio},
};

use crate::JodChild;

#[cfg(target_os = "scarlet")]
use scarlet_imp as imp;

pub fn streaming_output(
    out: ChildStdout,
    err: ChildStderr,
    on_stdout_line: &mut dyn FnMut(&str),
    on_stderr_line: &mut dyn FnMut(&str),
    on_eof: &mut dyn FnMut(),
) -> io::Result<(Vec<u8>, Vec<u8>)> {
    streaming_output_with(|data| imp::read2(out, err, data), on_stdout_line, on_stderr_line, on_eof)
}

fn streaming_output_with(
    read2: impl FnOnce(&mut dyn FnMut(bool, &mut Vec<u8>, bool)) -> io::Result<()>,
    on_stdout_line: &mut dyn FnMut(&str),
    on_stderr_line: &mut dyn FnMut(&str),
    on_eof: &mut dyn FnMut(),
) -> io::Result<(Vec<u8>, Vec<u8>)> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    read2(&mut |is_out, data, eof| {
        let idx = if eof {
            data.len()
        } else {
            match data.iter().rposition(|&b| b == b'\n') {
                Some(i) => i + 1,
                None => return,
            }
        };
        {
            // scope for new_lines
            let new_lines = {
                let dst = if is_out { &mut stdout } else { &mut stderr };
                let start = dst.len();
                let data = data.drain(..idx);
                dst.extend(data);
                &dst[start..]
            };
            for line in String::from_utf8_lossy(new_lines).lines() {
                if is_out {
                    on_stdout_line(line);
                } else {
                    on_stderr_line(line);
                }
            }
            if eof {
                on_eof();
            }
        }
    })?;

    Ok((stdout, stderr))
}

/// # Panics
///
/// Panics if `cmd` is not configured to have `stdout` and `stderr` as `piped`.
pub fn spawn_with_streaming_output(
    mut cmd: Command,
    on_stdout_line: &mut dyn FnMut(&str),
    on_stderr_line: &mut dyn FnMut(&str),
) -> io::Result<Output> {
    let cmd = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());

    let mut child = JodChild(cmd.spawn()?);
    let (stdout, stderr) = streaming_output(
        child.stdout.take().unwrap(),
        child.stderr.take().unwrap(),
        on_stdout_line,
        on_stderr_line,
        &mut || (),
    )?;
    let status = child.wait()?;
    Ok(Output { status, stdout, stderr })
}

// Native Scarlet pipes are blocking and do not expose poll or O_NONBLOCK.
// Keep the callbacks on the caller while two readers drain the pipes, as in
// Cargo's cargo-util/src/read2.rs Scarlet backend.
#[cfg(any(target_os = "scarlet", all(test, unix)))]
mod scarlet_imp {
    use std::{
        io::{self, Read},
        process::{ChildStderr, ChildStdout},
        sync::mpsc,
        thread,
    };

    enum Chunk {
        Bytes(bool, Vec<u8>),
        End(bool),
        Error(io::Error),
    }

    fn drain(mut pipe: impl Read, stdout: bool, sender: mpsc::SyncSender<Chunk>) {
        let mut buffer = [0u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => {
                    let _ = sender.send(Chunk::End(stdout));
                    return;
                }
                Ok(count) => {
                    if sender.send(Chunk::Bytes(stdout, buffer[..count].to_vec())).is_err() {
                        return;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    let _ = sender.send(Chunk::Error(error));
                    return;
                }
            }
        }
    }

    pub(crate) fn read2(
        out_pipe: ChildStdout,
        err_pipe: ChildStderr,
        data: &mut dyn FnMut(bool, &mut Vec<u8>, bool),
    ) -> io::Result<()> {
        let (sender, receiver) = mpsc::sync_channel(8);
        // Own the receiver inside the scope so returning or unwinding drops it
        // before joining readers that may be blocked on the bounded channel.
        thread::scope(move |scope| {
            let out_sender = sender.clone();
            let out_reader = scope.spawn(move || drain(out_pipe, true, out_sender));
            let err_reader = scope.spawn(move || drain(err_pipe, false, sender));
            let mut out = Vec::new();
            let mut err = Vec::new();
            let mut ended = 0;
            let mut first_error = None;
            while ended != 2 {
                match receiver.recv() {
                    Ok(Chunk::Bytes(stdout, bytes)) => {
                        let buffer = if stdout { &mut out } else { &mut err };
                        buffer.extend_from_slice(&bytes);
                        data(stdout, buffer, false);
                    }
                    Ok(Chunk::End(stdout)) => {
                        ended += 1;
                        data(stdout, if stdout { &mut out } else { &mut err }, true);
                    }
                    Ok(Chunk::Error(error)) => {
                        ended += 1;
                        first_error.get_or_insert(error);
                    }
                    Err(_) => return Err(io::ErrorKind::BrokenPipe.into()),
                }
            }
            out_reader.join().expect("stdout pipe reader panicked");
            err_reader.join().expect("stderr pipe reader panicked");
            first_error.map_or(Ok(()), Err)
        })
    }
}

#[cfg(all(unix, not(target_os = "scarlet")))]
mod imp {
    use std::{
        io::{self, prelude::*},
        mem,
        os::unix::prelude::*,
        process::{ChildStderr, ChildStdout},
    };

    pub(crate) fn read2(
        mut out_pipe: ChildStdout,
        mut err_pipe: ChildStderr,
        data: &mut dyn FnMut(bool, &mut Vec<u8>, bool),
    ) -> io::Result<()> {
        unsafe {
            libc::fcntl(out_pipe.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
            libc::fcntl(err_pipe.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
        }

        let mut out_done = false;
        let mut err_done = false;
        let mut out = Vec::new();
        let mut err = Vec::new();

        let mut fds: [libc::pollfd; 2] = unsafe { mem::zeroed() };
        fds[0].fd = out_pipe.as_raw_fd();
        fds[0].events = libc::POLLIN;
        fds[1].fd = err_pipe.as_raw_fd();
        fds[1].events = libc::POLLIN;
        let mut nfds = 2;
        let mut errfd = 1;

        while nfds > 0 {
            // wait for either pipe to become readable using `select`
            let r = unsafe { libc::poll(fds.as_mut_ptr(), nfds, -1) };
            if r == -1 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }

            // Read as much as we can from each pipe, ignoring EWOULDBLOCK or
            // EAGAIN. If we hit EOF, then this will happen because the underlying
            // reader will return Ok(0), in which case we'll see `Ok` ourselves. In
            // this case we flip the other fd back into blocking mode and read
            // whatever's leftover on that file descriptor.
            let handle = |res: io::Result<_>| match res {
                Ok(_) => Ok(true),
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        Ok(false)
                    } else {
                        Err(e)
                    }
                }
            };
            if !err_done && fds[errfd].revents != 0 && handle(err_pipe.read_to_end(&mut err))? {
                err_done = true;
                nfds -= 1;
            }
            data(false, &mut err, err_done);
            if !out_done && fds[0].revents != 0 && handle(out_pipe.read_to_end(&mut out))? {
                out_done = true;
                fds[0].fd = err_pipe.as_raw_fd();
                errfd = 0;
                nfds -= 1;
            }
            data(true, &mut out, out_done);
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        process::{Command, Stdio},
        sync::mpsc,
        thread,
        time::Duration,
    };

    #[test]
    fn scarlet_blocking_pipes_stream_lines_and_eof() {
        // Alternating more than a pipe's capacity on both streams deadlocks a
        // reader that drains either stream to EOF before starting the other.
        let out_line = "o".repeat(2048);
        let err_line = "e".repeat(2048);
        let mut child = Command::new("sh")
            .args([
                "-c",
                r#"
                    i=0
                    while [ "$i" -lt 2048 ]; do
                        printf '%s\n' "$1"
                        printf '%s\n' "$2" >&2
                        i=$((i+1))
                    done
                    printf 'stdout-tail'
                    exec 1>&-
                    printf 'stderr-tail' >&2
                "#,
                "stdx-pipe-test",
                &out_line,
                &err_line,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let out = child.stdout.take().unwrap();
        let err = child.stderr.take().unwrap();

        // Kill the writer on timeout so a sequential-reader regression fails
        // instead of leaving the test suite blocked on a full pipe.
        let (done, receiver) = mpsc::channel();
        let watchdog = thread::spawn(move || {
            let timed_out = matches!(
                receiver.recv_timeout(Duration::from_secs(30)),
                Err(mpsc::RecvTimeoutError::Timeout)
            );
            if timed_out {
                child.kill().unwrap();
            }
            (timed_out, child.wait().unwrap())
        });
        let mut out_lines = Vec::new();
        let mut err_lines = Vec::new();
        let mut eof_count = 0;
        // Exercise the native backend on the host, including the production
        // line splitter, without replacing the host's normal poll backend.
        let result = super::streaming_output_with(
            |data| super::scarlet_imp::read2(out, err, data),
            &mut |line| out_lines.push(line.to_owned()),
            &mut |line| err_lines.push(line.to_owned()),
            &mut || eof_count += 1,
        );
        let _ = done.send(());
        let (timed_out, status) = watchdog.join().unwrap();
        assert!(!timed_out, "blocking pipe readers deadlocked");
        assert!(status.success());
        let (stdout, stderr) = result.unwrap();
        assert_eq!(eof_count, 2);
        for (bytes, lines, line, tail) in [
            (stdout, out_lines, out_line, "stdout-tail"),
            (stderr, err_lines, err_line, "stderr-tail"),
        ] {
            let mut expected = format!("{line}\n").repeat(2048).into_bytes();
            expected.extend_from_slice(tail.as_bytes());
            assert_eq!(bytes.len(), expected.len());
            assert!(bytes == expected, "pipe output was corrupted");
            assert_eq!(lines.len(), 2049);
            assert!(lines[..2048].iter().all(|actual| *actual == line));
            assert_eq!(lines[2048], tail);
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::{
        io,
        os::windows::prelude::*,
        process::{ChildStderr, ChildStdout},
        slice,
    };

    use miow::{
        Overlapped,
        iocp::{CompletionPort, CompletionStatus},
        pipe::NamedPipe,
    };
    use windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE;

    struct Pipe<'a> {
        dst: &'a mut Vec<u8>,
        overlapped: Overlapped,
        pipe: NamedPipe,
        done: bool,
    }

    pub(crate) fn read2(
        out_pipe: ChildStdout,
        err_pipe: ChildStderr,
        data: &mut dyn FnMut(bool, &mut Vec<u8>, bool),
    ) -> io::Result<()> {
        let mut out = Vec::new();
        let mut err = Vec::new();

        let port = CompletionPort::new(1)?;
        port.add_handle(0, &out_pipe)?;
        port.add_handle(1, &err_pipe)?;

        unsafe {
            let mut out_pipe = Pipe::new(out_pipe, &mut out);
            let mut err_pipe = Pipe::new(err_pipe, &mut err);

            out_pipe.read()?;
            err_pipe.read()?;

            let mut status = [CompletionStatus::zero(), CompletionStatus::zero()];

            while !out_pipe.done || !err_pipe.done {
                for status in port.get_many(&mut status, None)? {
                    if status.token() == 0 {
                        out_pipe.complete(status);
                        data(true, out_pipe.dst, out_pipe.done);
                        out_pipe.read()?;
                    } else {
                        err_pipe.complete(status);
                        data(false, err_pipe.dst, err_pipe.done);
                        err_pipe.read()?;
                    }
                }
            }

            Ok(())
        }
    }

    impl<'a> Pipe<'a> {
        unsafe fn new<P: IntoRawHandle>(p: P, dst: &'a mut Vec<u8>) -> Pipe<'a> {
            let pipe = unsafe { NamedPipe::from_raw_handle(p.into_raw_handle()) };
            Pipe { dst, pipe, overlapped: Overlapped::zero(), done: false }
        }

        unsafe fn read(&mut self) -> io::Result<()> {
            let dst = unsafe { slice_to_end(self.dst) };
            match unsafe { self.pipe.read_overlapped(dst, self.overlapped.raw()) } {
                Ok(_) => Ok(()),
                Err(e) => {
                    if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                        self.done = true;
                        Ok(())
                    } else {
                        Err(e)
                    }
                }
            }
        }

        unsafe fn complete(&mut self, status: &CompletionStatus) {
            let prev = self.dst.len();
            unsafe { self.dst.set_len(prev + status.bytes_transferred() as usize) };
            if status.bytes_transferred() == 0 {
                self.done = true;
            }
        }
    }

    unsafe fn slice_to_end(v: &mut Vec<u8>) -> &mut [u8] {
        if v.capacity() == 0 {
            v.reserve(16);
        }
        if v.capacity() == v.len() {
            v.reserve(1);
        }
        let data = unsafe { v.as_mut_ptr().add(v.len()) };
        let len = v.capacity() - v.len();
        unsafe { slice::from_raw_parts_mut(data, len) }
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::{
        io,
        process::{ChildStderr, ChildStdout},
    };

    pub(crate) fn read2(
        _out_pipe: ChildStdout,
        _err_pipe: ChildStderr,
        _data: &mut dyn FnMut(bool, &mut Vec<u8>, bool),
    ) -> io::Result<()> {
        panic!("no processes on wasm")
    }
}
