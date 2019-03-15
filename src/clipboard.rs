use std::io::{Read, Write};
use std::process;
use std::process::Command;

pub fn clipboard_copy(s: &str) -> Result<(), failure::Error> {
    let mut p = Command::new("pbcopy")
        .stdin(process::Stdio::piped())
        .spawn()
        .or_else(|_| {
            Command::new("win32yank")
                .arg("-i")
                .stdin(process::Stdio::piped())
                .spawn()
        })
        .or_else(|_| {
            Command::new("win32yank.exe")
                .arg("-i")
                .stdin(process::Stdio::piped())
                .spawn()
        })
        .or_else(|_| {
            Command::new("xsel")
                .arg("-bi")
                .stdin(process::Stdio::piped())
                .spawn()
        })
        .or_else(|_| {
            Command::new("xclip")
                .arg("-i")
                .stdin(process::Stdio::piped())
                .spawn()
        })?;
    {
        let mut stdin = p.stdin.take().unwrap();
        write!(stdin, "{}", s)?;
    }
    p.wait()?;
    Ok(())
}

pub fn clipboard_paste() -> Option<String> {
    let p = Command::new("pbpaste")
        .stdout(process::Stdio::piped())
        .spawn()
        .or_else(|_| {
            Command::new("win32yank")
                .arg("-o")
                .stdout(process::Stdio::piped())
                .spawn()
        })
        .or_else(|_| {
            Command::new("win32yank.exe")
                .arg("-o")
                .stdout(process::Stdio::piped())
                .spawn()
        })
        .or_else(|_| {
            Command::new("xsel")
                .arg("-bo")
                .stdout(process::Stdio::piped())
                .spawn()
        })
        .or_else(|_| {
            Command::new("xclip")
                .arg("-o")
                .stdout(process::Stdio::piped())
                .spawn()
        })
        .ok()?;
    let mut stdout = p.stdout?;
    let mut buf = String::new();
    stdout.read_to_string(&mut buf).ok()?;
    Some(buf)
}
