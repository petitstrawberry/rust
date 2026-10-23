//! Standalone std-only native acceptance binary; build with the updated std.
//! Old fork backend: isolation's EOF deadline expires while the child is blocked.
use std::{collections::BTreeMap, env, fs, io::{self, BufRead, Read, Write},
    process::{self, Command, Stdio}, sync::mpsc, thread, time::{Duration, SystemTime, UNIX_EPOCH}};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn environment() -> BTreeMap<String, String> { env::vars().collect() }
fn child(args: &[String]) -> Result<()> {
    match args[1].as_str() {
        "--hold" => {
            io::stdout().write_all(b"READY\n")?;
            io::stdout().flush()?;
            let mut release = [0];
            io::stdin().read_exact(&mut release)?;
            assert_eq!(release, *b"R");
            io::stdout().write_all(b"DONE\n")?;
        }
        "--inherit" => print!("{:?}", environment()),
        "--script" => {
            assert_eq!(std::path::PathBuf::from(&args[0]), env::current_exe()?);
            assert_eq!(&args[2..], &["./entry-script", "script arg", ""]);
            print!("SCRIPT_OK\n");
        }
        "--script-chain" => print!("{args:?}"),
        "--closed-parent" => {
            use std::os::fd::{FromRawFd, OwnedFd};
            let exe = env::current_exe()?;
            let fd: i32 = args[2].parse()?;
            assert!(fd == 1 || fd == 2);
            // Dedicated child: transfer ownership of its standard descriptor
            // only to close it. The main test process retains its own streams.
            unsafe { drop(OwnedFd::from_raw_fd(fd)); }
            let mut bad = Command::new(&exe);
            bad.arg("--inherit");
            if fd == 1 { bad.stdout(Stdio::null()).stderr(io::stdout()); }
            else { bad.stderr(Stdio::null()).stdout(io::stderr()); }
            assert!(bad.spawn().is_err(), "redirecting from a closed stream must fail");
            assert!(Command::new(&exe).args(["--closed-check", &args[2]]).status()?.success());
        }
        "--closed-check" => {
            let result = if args[2] == "1" { io::stdout().write_all(b"must stay closed\n") }
                else { io::stderr().write_all(b"must stay closed\n") };
            assert!(result.is_err(), "inherited closed stream was reopened");
        }
        "--echo" => {
            assert_eq!(args[0], "spawn-child");
            assert_eq!(&args[2..5], &["space arg", "", "λ"]);
            assert_eq!(args.len(), 6);
            assert_eq!(env::current_dir()?, std::path::PathBuf::from(&args[5]));
            assert_eq!(environment(), BTreeMap::from([
                ("PATH".into(), "bin".into()), ("SPAWN_TOKEN".into(), "value = λ".into())]));
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input)?;
            assert_eq!(input, b"input\0bytes\n");
            io::stdout().write_all(b"OUT\0exact\n")?;
            io::stderr().write_all(b"ERR\0exact\n")?;
        }
        _ => panic!("unknown child mode"),
    }
    Ok(())
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    if args.len() > 1 { return child(&args); }
    let exe = env::current_exe()?;
    // Handshake guarantees the unrelated process is alive after exec, before EOF.
    let (mut reader, writer) = io::pipe()?;
    let mut blocker = Command::new(&exe).arg("--hold").stdin(Stdio::piped())
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut output = io::BufReader::new(blocker.stdout.take().unwrap());
    let mut ready = String::new();
    output.read_line(&mut ready)?;
    assert_eq!(ready, "READY\n");
    assert!(blocker.try_wait()?.is_none());
    drop(writer);
    let (send, recv) = mpsc::channel();
    let reading = thread::spawn(move || {
        let mut bytes = Vec::new();
        send.send(reader.read_to_end(&mut bytes).map(|_| bytes)).unwrap();
    });
    let eof = recv.recv_timeout(Duration::from_secs(10));
    assert!(blocker.try_wait()?.is_none());
    // Release even on an EOF timeout, so the failure does not strand the blocker.
    blocker.stdin.take().unwrap().write_all(b"R")?;
    let mut done = String::new();
    output.read_to_string(&mut done)?;
    assert_eq!(done, "DONE\n");
    let held = blocker.wait_with_output()?;
    assert!(held.status.success());
    assert!(held.stderr.is_empty());
    reading.join().unwrap();
    assert!(eof.map_err(|_| "unrelated child retained pipe writer: EOF timeout")??.is_empty());
    eprintln!("PIPE_ISOLATION_OK");
    let original_cwd = env::current_dir()?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::path::PathBuf::from(format!("/tmp/scarlet-spawn-{}-{stamp}", process::id()));
    fs::create_dir_all(root.join("work/bin"))?;
    fs::copy(&exe, root.join("work/bin/spawn-child"))?;
    env::set_current_dir(&root)?;
    let inherited = Command::new(&exe).arg("--inherit").output()?;
    assert!(inherited.status.success());
    assert!(inherited.stderr.is_empty());
    assert_eq!(inherited.stdout, format!("{:?}", environment()).as_bytes());
    assert!(Command::new(root.join("missing-executable")).stdin(Stdio::piped())
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().is_err());
    let mut echo = Command::new("spawn-child").args(["--echo", "space arg", "", "λ"])
        .arg(root.join("work")).current_dir("work").env_clear().env("PATH", "bin")
        .env("SPAWN_TOKEN", "value = λ").env("SPAWN_DROP", "remove me").env_remove("SPAWN_DROP")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    echo.stdin.take().unwrap().write_all(b"input\0bytes\n")?;
    let echoed = echo.wait_with_output()?;
    assert!(echoed.status.success());
    assert_eq!(echoed.stdout, b"OUT\0exact\n");
    assert_eq!(echoed.stderr, b"ERR\0exact\n");
    let interpreter = root.join("work/bin/spawn-child");
    fs::write(root.join("work/entry-script"),
        format!("#!{} --script ignored words\nunused body\n", interpreter.display()))?;
    let scripted = Command::new("./entry-script").current_dir("work")
        .args(["script arg", ""]).output()?;
    assert!(scripted.status.success(), "{:?}", scripted.stderr);
    assert_eq!(scripted.stdout, b"SCRIPT_OK\n");
    assert!(scripted.stderr.is_empty());
    let middle = root.join("work/middle-script");
    fs::write(&middle, format!("#!{} --script-chain ignored\n", interpreter.display()))?;
    fs::write(root.join("work/outer-script"), format!("#!{} outer-option ignored\n", middle.display()))?;
    let chain = Command::new("./outer-script").current_dir("work").args(["tail", ""]).output()?;
    assert!(chain.status.success(), "{:?}", chain.stderr);
    assert!(chain.stderr.is_empty());
    let expected = vec![interpreter.display().to_string(), "--script-chain".into(),
        middle.display().to_string(), "outer-option".into(), "./outer-script".into(), "tail".into(), "".into()];
    assert_eq!(chain.stdout, format!("{expected:?}").as_bytes());
    let third = root.join("work/third-script");
    fs::write(&third, format!("#!{}\n", root.join("work/outer-script").display()))?;
    assert!(Command::new(&third).output()?.status.success()); // three scripts + ELF
    let fourth = root.join("work/fourth-script");
    fs::write(&fourth, format!("#!{}\n", third.display()))?;
    assert!(Command::new(&fourth).spawn().is_err()); // a fifth image visit is rejected
    let looping = root.join("work/loop-script");
    fs::write(&looping, format!("#!{}\n", looping.display()))?;
    assert!(Command::new(&looping).spawn().is_err());
    fs::write(&looping, b"#!relative-interpreter\n")?;
    assert!(Command::new(&looping).spawn().is_err());
    for fd in ["1", "2"] {
        let closed = Command::new(&exe).args(["--closed-parent", fd]).output()?;
        assert!(closed.status.success(), "{:?}", closed.stderr);
        assert!(closed.stdout.is_empty());
        assert!(closed.stderr.is_empty());
    }
    eprintln!("SHEBANG_AND_CLOSED_STDIO_OK");
    assert_eq!(env::current_dir()?, root);
    env::set_current_dir(original_cwd)?;
    fs::remove_dir_all(root)?;
    eprintln!("SCARLET_ENVIRONMENT_SPAWN_OK");
    Ok(())
}
fn main() {
    std::panic::set_hook(Box::new(|p| eprintln!("SCARLET_ENVIRONMENT_SPAWN_FAIL {p}")));
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(90));
        eprintln!("SCARLET_ENVIRONMENT_SPAWN_FAIL watchdog"); process::exit(1);
    });
    if let Err(e) = run() { eprintln!("SCARLET_ENVIRONMENT_SPAWN_FAIL {e}"); process::exit(1); }
}
