use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_delfaf")
}

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("delfaf-cli-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn pipe_deletes_listed_paths() {
    let root = tmp("pipe");
    let a = root.join("node_modules");
    let b = root.join("app").join("node_modules");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(a.join("x"), b"1").unwrap();
    fs::write(b.join("y"), b"2").unwrap();

    let list = format!("{}\n{}\n", a.display(), b.display());
    let mut child = Command::new(bin())
        .arg("-y")
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(list.as_bytes())
            .unwrap();
    }
    let status = child.wait_with_output().unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert!(!a.exists());
    assert!(!b.exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn file_arg_deletes_listed_paths() {
    let root = tmp("filearg");
    let nm = root.join("node_modules");
    fs::create_dir_all(&nm).unwrap();
    let list = root.join("hits.txt");
    fs::write(&list, format!("{}\n", nm.display())).unwrap();

    let output = Command::new(bin())
        .args(["-y"])
        .arg(&list)
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!nm.exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn directory_arg_gives_clear_error() {
    let dir = tmp("dirarg");
    let output = Command::new(bin())
        .arg(&dir)
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2));
    assert!(!stderr.contains("os error"), "{stderr}");
    assert!(stderr.contains("is a directory"), "{stderr}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn empty_input_exits_1() {
    let mut child = Command::new(bin())
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
}
