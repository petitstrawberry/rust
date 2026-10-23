use super::env::{CommandEnv, CommandEnvs};
pub use crate::ffi::OsString as EnvKey;
use crate::ffi::{CString, OsStr, OsString};
use crate::num::NonZero;
use crate::path::{Path, PathBuf};
use crate::process::StdioPipes;
use crate::sys::fs::{File, OpenOptions};
use crate::sys::pal::abi;
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
    Pipe(ChildPipe),
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
        // Observe closed inherited stdio before any temporary handle can reuse
        // 0, 1, or 2. EnvironmentSpawn inherits no ambient handles at all.
        let inherited = [
            abi::handle_is_open(abi::STDIN_HANDLE)?,
            abi::handle_is_open(abi::STDOUT_HANDLE)?,
            abi::handle_is_open(abi::STDERR_HANDLE)?,
        ];
        let (child_stdio, pipes) = self.setup_stdio(&default, needs_stdin, &inherited)?;
        let prepared = PreparedCommand::new(self)?;
        let mappings = child_stdio.mappings();
        let pid = prepared.spawn(&mappings)?;

        // The kernel retained its own endpoint references. Drop our child ends
        // before returning the parent pipes, including on every error above.
        drop(child_stdio);
        Ok((Process { pid: pid as i32 }, pipes))
    }

    fn setup_stdio(
        &self,
        default: &Stdio,
        needs_stdin: bool,
        inherited: &[bool; 3],
    ) -> io::Result<(ChildStdio, StdioPipes)> {
        let (stdin_child, stdin_parent) =
            setup_stdio_slot(self.stdin.as_ref(), default, needs_stdin, true, StdioRole::Stdin, inherited)?;
        let (stdout_child, stdout_parent) =
            setup_stdio_slot(self.stdout.as_ref(), default, true, false, StdioRole::Stdout, inherited)?;
        let (stderr_child, stderr_parent) =
            setup_stdio_slot(self.stderr.as_ref(), default, true, false, StdioRole::Stderr, inherited)?;

        Ok((
            ChildStdio { stdin: stdin_child, stdout: stdout_child, stderr: stderr_child },
            StdioPipes { stdin: stdin_parent, stdout: stdout_parent, stderr: stderr_parent },
        ))
    }
}

fn setup_stdio_slot(
    configured: Option<&Stdio>,
    default: &Stdio,
    use_default: bool,
    readable: bool,
    role: StdioRole,
    inherited: &[bool; 3],
) -> io::Result<(Option<ChildStdioHandle>, Option<ChildPipe>)> {
    let inherit = |source: usize| {
        (inherited[source].then_some(ChildStdioHandle::Inherited(source)), None)
    };
    let stdio = match configured {
        Some(stdio) => stdio,
        None if use_default => default,
        None => return Ok(inherit(role.handle())),
    };

    match (role, stdio) {
        (_, Stdio::Inherit) => Ok(inherit(role.handle())),
        // Inheriting a closed slot keeps it closed, but explicitly redirecting
        // another stream from that slot must fail, as duplicating it would.
        (_, Stdio::ParentStdout)
            if role.handle() != abi::STDOUT_HANDLE && !inherited[abi::STDOUT_HANDLE] =>
        {
            Err(io::Error::other("cannot redirect from closed parent stdout"))
        }
        (_, Stdio::ParentStderr)
            if role.handle() != abi::STDERR_HANDLE && !inherited[abi::STDERR_HANDLE] =>
        {
            Err(io::Error::other("cannot redirect from closed parent stderr"))
        }
        (_, Stdio::ParentStdout) => Ok(inherit(abi::STDOUT_HANDLE)),
        (_, Stdio::ParentStderr) => Ok(inherit(abi::STDERR_HANDLE)),
        (_, Stdio::Null) => Ok((Some(ChildStdioHandle::File(open_null_stdio(readable)?)), None)),
        (_, Stdio::Pipe(pipe)) => Ok((Some(ChildStdioHandle::Pipe(pipe.try_clone()?)), None)),
        (_, Stdio::InheritFile(file)) => {
            Ok((Some(ChildStdioHandle::File(file.duplicate()?)), None))
        }
        (_, Stdio::MakePipe) => {
            let (reader, writer) = crate::sys::pipe::pipe()?;
            let (parent, child) = if readable { (writer, reader) } else { (reader, writer) };
            Ok((Some(ChildStdioHandle::Pipe(child)), Some(parent)))
        }
    }
}

fn open_null_stdio(readable: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(readable);
    options.write(!readable);
    File::open(Path::new("/dev/null"), &options)
}

#[derive(Clone, Copy)]
enum StdioRole {
    Stdin,
    Stdout,
    Stderr,
}

impl StdioRole {
    fn handle(self) -> usize {
        match self {
            Self::Stdin => abi::STDIN_HANDLE,
            Self::Stdout => abi::STDOUT_HANDLE,
            Self::Stderr => abi::STDERR_HANDLE,
        }
    }
}

struct ChildStdio {
    stdin: Option<ChildStdioHandle>,
    stdout: Option<ChildStdioHandle>,
    stderr: Option<ChildStdioHandle>,
}

impl ChildStdio {
    fn mappings(&self) -> Vec<abi::RawEnvironmentHandleMapping> {
        let mut mappings = Vec::with_capacity(3);
        for (target, handle) in [
            (abi::STDIN_HANDLE, &self.stdin),
            (abi::STDOUT_HANDLE, &self.stdout),
            (abi::STDERR_HANDLE, &self.stderr),
        ] {
            if let Some(handle) = handle {
                mappings.push(abi::RawEnvironmentHandleMapping {
                    source: handle.as_raw_handle() as u32,
                    target: target as u32,
                });
            }
        }
        mappings
    }
}

enum ChildStdioHandle {
    Pipe(ChildPipe),
    File(File),
    // Borrowed parent stdio; the kernel duplicates it directly into the child.
    Inherited(usize),
}

impl ChildStdioHandle {
    fn as_raw_handle(&self) -> usize {
        match self {
            Self::Pipe(pipe) => pipe.as_raw_handle(),
            Self::File(file) => file.as_raw_handle(),
            Self::Inherited(handle) => *handle,
        }
    }
}

struct EnvironmentHandle(usize);

impl Drop for EnvironmentHandle {
    fn drop(&mut self) {
        let _ = abi::handle_close(self.0);
    }
}

struct PreparedCommand {
    executable: File,
    argv_data: Vec<CString>,
    argv: Vec<*const u8>,
    envp_data: Vec<CString>,
    envp: Vec<*const u8>,
    cwd: CString,
}

impl PreparedCommand {
    fn new(command: &Command) -> io::Result<Self> {
        let env = command.env.capture();
        let cwd = match command.cwd.as_deref().map(Path::new) {
            Some(cwd) if cwd.as_os_str().is_empty() => return Err(io::ErrorKind::InvalidInput.into()),
            Some(cwd) if cwd.is_absolute() => cwd.to_path_buf(),
            Some(cwd) => crate::env::current_dir()?.join(cwd),
            None => crate::env::current_dir()?,
        };
        let program = resolve_program(&command.program, &env, &cwd)?;
        let argv_data =
            command.args.iter().map(|arg| os_to_cstring(arg)).collect::<io::Result<Vec<_>>>()?;
        let (executable, argv_data) = Self::open_executable(program, &cwd, argv_data)?;
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

        let cwd = os_to_cstring(cwd.as_os_str())?;

        Ok(Self { executable, argv_data, argv, envp_data, envp, cwd })
    }

    fn open_executable(
        mut program: PathBuf,
        cwd: &Path,
        mut argv: Vec<CString>,
    ) -> io::Result<(File, Vec<CString>)> {
        let mut options = OpenOptions::new();
        options.read(true);
        // Match kernel TransparentExecutor::execute_path: four image visits,
        // a 256-byte prefix, an absolute interpreter, and at most one option.
        // EnvironmentSpawn accepts a held binary, so resolve scripts here.
        for _ in 0..4 {
            let executable = File::open(&cwd.join(&program), &options)?;
            let mut header = [0; 256];
            let n = executable.read(&mut header).unwrap_or(0);
            executable.seek(io::SeekFrom::Start(0))?;
            if n < 2 || &header[..2] != b"#!" {
                return Ok((executable, argv));
            }
            let end = header[..n].iter().position(|&b| b == b'\n').unwrap_or(n);
            let line = str::from_utf8(&header[2..end])
                .map_err(|_| io::Error::other("invalid script interpreter"))?;
            let mut words = line.split_whitespace();
            let interpreter = words.next().filter(|name| name.starts_with('/'))
                .ok_or_else(|| io::Error::other("invalid script interpreter"))?;
            let mut script_argv = vec![os_to_cstring(OsStr::new(interpreter))?];
            if let Some(option) = words.next() {
                script_argv.push(os_to_cstring(OsStr::new(option))?);
            }
            // Preserve the path spelling used to launch this script, including
            // relative PATH entries. ELF argv[0] is otherwise left untouched.
            script_argv.push(os_to_cstring(program.as_os_str())?);
            script_argv.extend(argv.into_iter().skip(1));
            argv = script_argv;
            program = PathBuf::from(interpreter);
        }
        Err(io::Error::other("runtime delegation loop"))
    }

    fn spawn(&self, handles: &[abi::RawEnvironmentHandleMapping]) -> io::Result<u32> {
        let environment = EnvironmentHandle(
            abi::environment_current().map_err(|()| io::Error::other("EnvironmentCurrent failed"))?,
        );
        let _keep_alive = (&self.argv_data, &self.envp_data);
        let options = abi::RawEnvironmentExec {
            size: crate::mem::size_of::<abi::RawEnvironmentExec>() as u32,
            flags: 0,
            argv: self.argv.as_ptr() as usize,
            envp: self.envp.as_ptr() as usize,
            cwd: self.cwd.as_ptr() as usize,
            handles: handles.as_ptr() as usize,
            handle_count: handles.len(),
        };
        // SAFETY: self owns all strings, pointer arrays and the executable;
        // the caller owns mapped stdio and the Environment guard owns its handle.
        // All remain live until the synchronous syscall has copied the inputs.
        unsafe {
            abi::environment_spawn(environment.0, self.executable.as_raw_handle(), &options)
        }
        .map_err(|()| io::Error::other("EnvironmentSpawn failed"))
    }
}

fn resolve_program(
    program: &OsStr,
    env: &crate::collections::BTreeMap<EnvKey, OsString>,
    cwd: &Path,
) -> io::Result<PathBuf> {
    if program.as_encoded_bytes().contains(&b'/') {
        return Ok(PathBuf::from(program));
    }

    if let Some(path) = env.get(OsStr::new("PATH")) {
        for dir in path.as_encoded_bytes().split(|b| *b == b':') {
            let mut candidate =
                PathBuf::from(str::from_utf8(dir).map_err(|_| io::ErrorKind::InvalidInput)?);
            candidate.push(program);
            if fs::File::open(cwd.join(&candidate)).is_ok() {
                return Ok(candidate);
            }
        }
    }

    let mut current_dir_candidate = PathBuf::from(".");
    current_dir_candidate.push(program);
    if fs::File::open(cwd.join(&current_dir_candidate)).is_ok() {
        return Ok(current_dir_candidate);
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

pub fn output(cmd: &mut Command) -> io::Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
    let (mut process, mut pipes) = cmd.spawn(Stdio::MakePipe, false)?;

    drop(pipes.stdin.take());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    match (pipes.stdout.take(), pipes.stderr.take()) {
        (None, None) => {}
        (Some(out), None) => {
            out.read_to_end(&mut stdout)?;
        }
        (None, Some(err)) => {
            err.read_to_end(&mut stderr)?;
        }
        (Some(out), Some(err)) => {
            read_output(out, &mut stdout, err, &mut stderr)?;
        }
    }

    let status = process.wait()?;
    Ok((status, stdout, stderr))
}

impl From<ChildPipe> for Stdio {
    fn from(pipe: ChildPipe) -> Stdio {
        Stdio::Pipe(pipe)
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
        unsupported_process("Child::kill is not supported until Native Kill is implemented")
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
    out: ChildPipe,
    stdout: &mut Vec<u8>,
    err: ChildPipe,
    stderr: &mut Vec<u8>,
) -> io::Result<()> {
    let stdout_reader = crate::thread::spawn(move || {
        let mut buffer = Vec::new();
        out.read_to_end(&mut buffer).map(|_| buffer)
    });
    let stderr_reader = crate::thread::spawn(move || {
        let mut buffer = Vec::new();
        err.read_to_end(&mut buffer).map(|_| buffer)
    });

    *stdout = join_pipe_reader(stdout_reader, "stdout reader thread panicked")?;
    *stderr = join_pipe_reader(stderr_reader, "stderr reader thread panicked")?;
    Ok(())
}

fn join_pipe_reader(
    reader: crate::thread::JoinHandle<io::Result<Vec<u8>>>,
    panic_message: &'static str,
) -> io::Result<Vec<u8>> {
    reader.join().map_err(|_| io::Error::new(io::ErrorKind::Other, panic_message))?
}

fn unsupported_process<T>(message: &'static str) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::Unsupported, message))
}
