use super::env::{CommandEnv, CommandEnvs};
pub use crate::ffi::OsString as EnvKey;
use crate::ffi::{CString, OsStr, OsString};
use crate::num::NonZero;
use crate::path::{Path, PathBuf};
use crate::process::StdioPipes;
use crate::sys::fs::File;
use crate::sys::pal::abi;
use crate::sys::unsupported;
use crate::{fmt, fs, io, str};

const WAIT_NOHANG: i32 = 0x1;

////////////////////////////////////////////////////////////////////////////////
// Command
////////////////////////////////////////////////////////////////////////////////

pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    env: CommandEnv,

    cwd: Option<OsString>,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
}

#[derive(Debug)]
pub enum Stdio {
    Inherit,
    Null,
    MakePipe,
    ParentStdout,
    ParentStderr,
    #[allow(dead_code)] // This variant exists only for the Debug impl for now.
    InheritFile(File),
}

impl Command {
    pub fn new(program: &OsStr) -> Command {
        Command {
            program: program.to_owned(),
            args: vec![program.to_owned()],
            env: Default::default(),
            cwd: None,
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    pub fn arg(&mut self, arg: &OsStr) {
        self.args.push(arg.to_owned());
    }

    pub fn env_mut(&mut self) -> &mut CommandEnv {
        &mut self.env
    }

    pub fn cwd(&mut self, dir: &OsStr) {
        self.cwd = Some(dir.to_owned());
    }

    pub fn stdin(&mut self, stdin: Stdio) {
        self.stdin = Some(stdin);
    }

    pub fn stdout(&mut self, stdout: Stdio) {
        self.stdout = Some(stdout);
    }

    pub fn stderr(&mut self, stderr: Stdio) {
        self.stderr = Some(stderr);
    }

    pub fn get_program(&self) -> &OsStr {
        &self.program
    }

    pub fn get_args(&self) -> CommandArgs<'_> {
        let mut iter = self.args.iter();
        iter.next();
        CommandArgs { iter }
    }

    pub fn get_envs(&self) -> CommandEnvs<'_> {
        self.env.iter()
    }

    pub fn get_env_clear(&self) -> bool {
        self.env.does_clear()
    }

    pub fn get_current_dir(&self) -> Option<&Path> {
        self.cwd.as_ref().map(|cs| Path::new(cs))
    }

    pub fn spawn(
        &mut self,
        default: Stdio,
        needs_stdin: bool,
    ) -> io::Result<(Process, StdioPipes)> {
        validate_stdio(self.stdin.as_ref(), &default, needs_stdin, StdioRole::Stdin)?;
        validate_stdio(self.stdout.as_ref(), &default, true, StdioRole::Stdout)?;
        validate_stdio(self.stderr.as_ref(), &default, true, StdioRole::Stderr)?;

        let prepared = PreparedCommand::new(self)?;
        let pid = abi::clone_process(0).map_err(|()| io::ErrorKind::Other)?;

        if pid == 0 {
            prepared.exec_in_child();
        }

        Ok((Process { pid: pid as i32 }, StdioPipes { stdin: None, stdout: None, stderr: None }))
    }
}

fn validate_stdio(
    configured: Option<&Stdio>,
    default: &Stdio,
    use_default: bool,
    role: StdioRole,
) -> io::Result<()> {
    let stdio = match configured {
        Some(stdio) => stdio,
        None if use_default => default,
        None => return Ok(()),
    };

    match (role, stdio) {
        (_, Stdio::Inherit) => Ok(()),
        (StdioRole::Stdout, Stdio::ParentStdout) => Ok(()),
        (StdioRole::Stderr, Stdio::ParentStderr) => Ok(()),
        // TODO(scarlet): support Null, MakePipe, InheritFile, and cross-stream
        // ParentStdout/ParentStderr once Native has handle remapping semantics
        // for child stdio setup.
        _ => unsupported(),
    }
}

#[derive(Clone, Copy)]
enum StdioRole {
    Stdin,
    Stdout,
    Stderr,
}

struct PreparedCommand {
    program: CString,
    argv_data: Vec<CString>,
    argv: Vec<*const u8>,
    envp_data: Vec<CString>,
    envp: Vec<*const u8>,
    cwd: Option<CString>,
}

impl PreparedCommand {
    fn new(command: &Command) -> io::Result<Self> {
        let env = command.env.capture();
        let program = resolve_program(&command.program, &env)?;
        let program = os_to_cstring(&program)?;

        let argv_data =
            command.args.iter().map(|arg| os_to_cstring(arg)).collect::<io::Result<Vec<_>>>()?;
        let argv = ptr_array(&argv_data);

        let mut envp_data = Vec::with_capacity(env.len());
        for (key, value) in env {
            let mut entry = Vec::new();
            entry.extend_from_slice(key.as_encoded_bytes());
            entry.push(b'=');
            entry.extend_from_slice(value.as_encoded_bytes());
            envp_data.push(bytes_to_cstring(entry)?);
        }
        let envp = ptr_array(&envp_data);

        let cwd = command.cwd.as_ref().map(|cwd| os_to_cstring(cwd)).transpose()?;

        Ok(Self { program, argv_data, argv, envp_data, envp, cwd })
    }

    fn exec_in_child(&self) -> ! {
        if let Some(cwd) = &self.cwd
            && abi::vfs_change_directory(cwd.as_ptr().cast()).is_err()
        {
            abi::exit_group(127);
        }

        let _keep_alive = (&self.argv_data, &self.envp_data);
        if abi::execve(self.program.as_ptr().cast(), self.argv.as_ptr(), self.envp.as_ptr())
            .is_err()
        {
            abi::exit_group(127);
        }

        loop {
            core::hint::spin_loop();
        }
    }
}

fn resolve_program(
    program: &OsStr,
    env: &crate::collections::BTreeMap<EnvKey, OsString>,
) -> io::Result<OsString> {
    if program.as_encoded_bytes().contains(&b'/') {
        return Ok(program.to_owned());
    }

    if let Some(path) = env.get(OsStr::new("PATH")) {
        for dir in path.as_encoded_bytes().split(|b| *b == b':') {
            if dir.is_empty() {
                continue;
            }
            let mut candidate =
                PathBuf::from(str::from_utf8(dir).map_err(|_| io::ErrorKind::InvalidInput)?);
            candidate.push(program);
            if fs::File::open(&candidate).is_ok() {
                return Ok(candidate.into_os_string());
            }
        }
    }

    let mut current_dir_candidate = PathBuf::from(".");
    current_dir_candidate.push(program);
    if fs::File::open(&current_dir_candidate).is_ok() {
        return Ok(current_dir_candidate.into_os_string());
    }

    Err(io::ErrorKind::NotFound.into())
}

fn ptr_array(strings: &[CString]) -> Vec<*const u8> {
    let mut ptrs = strings.iter().map(|string| string.as_ptr().cast()).collect::<Vec<_>>();
    ptrs.push(core::ptr::null());
    ptrs
}

fn os_to_cstring(value: &OsStr) -> io::Result<CString> {
    bytes_to_cstring(value.as_encoded_bytes().to_vec())
}

fn bytes_to_cstring(value: Vec<u8>) -> io::Result<CString> {
    str::from_utf8(&value).map_err(|_| io::ErrorKind::InvalidInput)?;
    CString::new(value).map_err(|_| io::ErrorKind::InvalidInput.into())
}

pub fn output(_cmd: &mut Command) -> io::Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
    // TODO(scarlet): implement this with Stdio::MakePipe once child stdio
    // handle remapping is available in the Native ABI.
    unsupported()
}

impl From<ChildPipe> for Stdio {
    fn from(pipe: ChildPipe) -> Stdio {
        pipe.diverge()
    }
}

impl From<io::Stdout> for Stdio {
    fn from(_: io::Stdout) -> Stdio {
        Stdio::ParentStdout
    }
}

impl From<io::Stderr> for Stdio {
    fn from(_: io::Stderr) -> Stdio {
        Stdio::ParentStderr
    }
}

impl From<File> for Stdio {
    fn from(file: File) -> Stdio {
        Stdio::InheritFile(file)
    }
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            let mut debug_command = f.debug_struct("Command");
            debug_command.field("program", &self.program).field("args", &self.args);
            if !self.env.is_unchanged() {
                debug_command.field("env", &self.env);
            }

            if self.cwd.is_some() {
                debug_command.field("cwd", &self.cwd);
            }
            if self.stdin.is_some() {
                debug_command.field("stdin", &self.stdin);
            }
            if self.stdout.is_some() {
                debug_command.field("stdout", &self.stdout);
            }
            if self.stderr.is_some() {
                debug_command.field("stderr", &self.stderr);
            }

            debug_command.finish()
        } else {
            if let Some(ref cwd) = self.cwd {
                write!(f, "cd {cwd:?} && ")?;
            }
            if self.env.does_clear() {
                write!(f, "env -i ")?;
            } else {
                let mut any_removed = false;
                for (key, value_opt) in self.get_envs() {
                    if value_opt.is_none() {
                        if !any_removed {
                            write!(f, "env ")?;
                            any_removed = true;
                        }
                        write!(f, "-u {} ", key.to_string_lossy())?;
                    }
                }
            }
            for (key, value_opt) in self.get_envs() {
                if let Some(value) = value_opt {
                    write!(f, "{}={value:?} ", key.to_string_lossy())?;
                }
            }
            if self.program != self.args[0] {
                write!(f, "[{:?}] ", self.program)?;
            }
            write!(f, "{:?}", self.args[0])?;

            for arg in &self.args[1..] {
                write!(f, " {:?}", arg)?;
            }
            Ok(())
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Default)]
pub struct ExitStatus(i32);

impl ExitStatus {
    pub fn exit_ok(&self) -> Result<(), ExitStatusError> {
        if self.0 == 0 { Ok(()) } else { Err(ExitStatusError(*self)) }
    }

    pub fn code(&self) -> Option<i32> {
        Some(self.0)
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exit code: {}", self.0)
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct ExitStatusError(ExitStatus);

impl Into<ExitStatus> for ExitStatusError {
    fn into(self) -> ExitStatus {
        self.0
    }
}

impl ExitStatusError {
    pub fn code(self) -> Option<NonZero<i32>> {
        NonZero::new(self.0.0)
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct ExitCode(u8);

impl ExitCode {
    pub const SUCCESS: ExitCode = ExitCode(0);
    pub const FAILURE: ExitCode = ExitCode(1);

    pub fn as_i32(&self) -> i32 {
        self.0 as i32
    }
}

impl From<u8> for ExitCode {
    fn from(code: u8) -> Self {
        Self(code)
    }
}

pub struct Process {
    pid: i32,
}

impl Process {
    pub fn id(&self) -> u32 {
        self.pid as u32
    }

    pub fn kill(&mut self) -> io::Result<()> {
        // TODO(scarlet): wire this to Native Kill once syscall 6 has a kernel
        // implementation.
        unsupported()
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let mut status = 0;
        match abi::waitpid(self.pid, &mut status, 0) {
            Ok(pid) if pid == self.pid => Ok(ExitStatus(status)),
            Ok(_) => Err(io::ErrorKind::Other.into()),
            Err(()) => Err(io::ErrorKind::Other.into()),
        }
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let mut status = 0;
        match abi::waitpid(self.pid, &mut status, WAIT_NOHANG) {
            Ok(0) => Ok(None),
            Ok(pid) if pid == self.pid => Ok(Some(ExitStatus(status))),
            Ok(_) => Err(io::ErrorKind::Other.into()),
            Err(()) => Err(io::ErrorKind::Other.into()),
        }
    }
}

pub struct CommandArgs<'a> {
    iter: crate::slice::Iter<'a, OsString>,
}

impl<'a> Iterator for CommandArgs<'a> {
    type Item = &'a OsStr;
    fn next(&mut self) -> Option<&'a OsStr> {
        self.iter.next().map(|os| &**os)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a> ExactSizeIterator for CommandArgs<'a> {
    fn len(&self) -> usize {
        self.iter.len()
    }
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<'a> fmt::Debug for CommandArgs<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter.clone()).finish()
    }
}

pub type ChildPipe = crate::sys::pipe::Pipe;

pub fn read_output(
    _out: ChildPipe,
    _stdout: &mut Vec<u8>,
    _err: ChildPipe,
    _stderr: &mut Vec<u8>,
) -> io::Result<()> {
    // TODO(scarlet): implement concurrent pipe draining for Command::output().
    unsupported()
}
