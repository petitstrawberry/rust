use std::io::{self, Read, Write};
use std::path::Path;
use std::{env, fs};

fn main() -> io::Result<()> {
    println!("scarlet-rust-std-m3-smoke: start pid={}", std::process::id());

    let args = env::args().collect::<Vec<_>>();
    println!("args={args:?}");
    if args.is_empty() {
        return Err(io::Error::other("argv was not initialized"));
    }

    unsafe {
        env::set_var("SCARLET_STD_M3_SMOKE", "ok");
    }
    if env::var("SCARLET_STD_M3_SMOKE").as_deref() != Ok("ok") {
        return Err(io::Error::other("environment variable roundtrip failed"));
    }

    let original_cwd = env::current_dir()?;
    println!("cwd={}", original_cwd.display());
    io::stdout().write_all(b"stdout=ok\n")?;
    io::stderr().write_all(b"stderr=ok\n")?;

    let root_names = sorted_dir_names("/")?;
    println!("root_read_dir={root_names:?}");
    if !root_names.iter().any(|name| name == "tmp") {
        return Err(io::Error::other("root read_dir did not return tmp"));
    }
    if !root_names.iter().any(|name| name == "bin") {
        return Err(io::Error::other("root read_dir did not return bin"));
    }

    let dev_names = sorted_dir_names("/dev")?;
    println!("dev_read_dir={dev_names:?}");
    if !dev_names.iter().any(|name| name == "tty0") {
        return Err(io::Error::other("devfs read_dir did not return tty0"));
    }
    if !dev_names.iter().any(|name| name == "null") {
        return Err(io::Error::other("devfs read_dir did not return null"));
    }

    let dev_null = fs::metadata("/dev/null")?;
    if dev_null.is_dir() {
        return Err(io::Error::other("/dev/null metadata had unexpected file type"));
    }

    let (mut pipe_reader, mut pipe_writer) = io::pipe()?;
    pipe_writer.write_all(b"pipe-roundtrip")?;
    drop(pipe_writer);
    let mut pipe_data = String::new();
    pipe_reader.read_to_string(&mut pipe_data)?;
    if pipe_data != "pipe-roundtrip" {
        return Err(io::Error::other("pipe roundtrip failed"));
    }

    let root = Path::new("/tmp/scarlet-std-m3-smoke");
    let _ = fs::remove_file(root.join("link.txt"));
    let _ = fs::remove_file(root.join("renamed.txt"));
    let _ = fs::remove_file(root.join("hello.txt"));
    let _ = fs::remove_dir(root);

    fs::create_dir(root)?;
    env::set_current_dir(root)?;
    if env::current_dir()? != root {
        return Err(io::Error::other("set_current_dir did not update cwd"));
    }

    fs::write("hello.txt", "hello from Scarlet std\n")?;
    let data = fs::read_to_string("hello.txt")?;
    if data != "hello from Scarlet std\n" {
        return Err(io::Error::other("read_to_string returned unexpected data"));
    }

    let metadata = fs::metadata("hello.txt")?;
    if !metadata.is_file() || metadata.len() != data.len() as u64 {
        return Err(io::Error::other("metadata for hello.txt was inconsistent"));
    }

    fs::rename("hello.txt", "renamed.txt")?;
    fs::hard_link("renamed.txt", "link.txt")?;
    fs::remove_file("renamed.txt")?;
    if fs::read_to_string("link.txt")? != data {
        return Err(io::Error::other("hardlink content changed after unlink"));
    }

    let names = sorted_dir_names(".")?;
    if !names.iter().any(|name| name == "link.txt") {
        return Err(io::Error::other("read_dir did not return link.txt"));
    }
    println!("read_dir={names:?}");

    env::set_current_dir(&original_cwd)?;
    fs::remove_file(root.join("link.txt"))?;
    fs::remove_dir(root)?;

    unsafe {
        env::remove_var("SCARLET_STD_M3_SMOKE");
    }

    println!("scarlet-rust-std-m3-smoke: ok");
    Ok(())
}

fn sorted_dir_names(path: impl AsRef<Path>) -> io::Result<Vec<String>> {
    let mut names = fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<io::Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}
