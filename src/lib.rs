use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

pub fn parse_paths(bytes: &[u8]) -> Vec<PathBuf> {
    let cleaned = strip_ansi(bytes);
    let sep = if cleaned.contains(&0) { 0u8 } else { b'\n' };
    cleaned
        .split(|b| *b == sep)
        .filter_map(|raw| {
            let line = trim_path_bytes(raw);
            if line.is_empty() {
                return None;
            }
            Some(bytes_to_path(line))
        })
        .collect()
}

fn strip_ansi(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            while i < bytes.len() && !(bytes[i] >= b'@' && bytes[i] <= b'~') {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn trim_path_bytes(raw: &[u8]) -> &[u8] {
    let mut line = raw;
    if let Some(b'\r') = line.last() {
        line = &line[..line.len() - 1];
    }
    line
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).as_ref())
}

pub fn last_cache_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FAFIND_LAST") {
        return Some(PathBuf::from(p));
    }
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(xdg).join("fafind").join("last"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/fafind/last"))
}

pub fn is_forbidden(path: &Path) -> bool {
    use std::path::Component;
    let mut normals = 0usize;
    for c in path.components() {
        match c {
            Component::ParentDir => return true,
            Component::Normal(_) => normals += 1,
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
        }
    }
    normals == 0
}

pub fn delete_path(path: &Path) -> io::Result<bool> {
    if is_forbidden(path) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to delete",
        ));
    }

    let meta = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };

    let ft = meta.file_type();
    let removed = {
        #[cfg(windows)]
        {
            if ft.is_symlink() && ft.is_dir() {
                std::fs::remove_dir(path)
            } else if ft.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            }
        }
        #[cfg(not(windows))]
        {
            if ft.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            }
        }
    };
    match removed {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

pub fn delete_paths(paths: &[PathBuf]) -> (u64, u64) {
    if paths.len() <= 1 {
        return delete_chunk(paths);
    }
    let n = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
        .min(paths.len());
    let chunk = paths.len().div_ceil(n);
    std::thread::scope(|s| {
        let mut ok = 0u64;
        let mut err = 0u64;
        let handles: Vec<_> = paths
            .chunks(chunk)
            .map(|c| s.spawn(|| delete_chunk(c)))
            .collect();
        for h in handles {
            let (o, e) = h.join().expect("delete worker");
            ok += o;
            err += e;
        }
        (ok, err)
    })
}

fn delete_chunk(paths: &[PathBuf]) -> (u64, u64) {
    let mut ok = 0u64;
    let mut err = 0u64;
    for p in paths {
        match delete_path(p) {
            Ok(true) => ok += 1,
            Ok(false) => {}
            Err(e) => {
                eprintln!("delfaf: {}: {e}", p.to_string_lossy().escape_default());
                err += 1;
            }
        }
    }
    (ok, err)
}

pub fn load_input(file: Option<&Path>) -> io::Result<Vec<u8>> {
    if let Some(path) = file {
        return std::fs::read(path).map_err(|e| annotate_directory(e, path));
    }
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        let mut buf = Vec::new();
        stdin.lock().read_to_end(&mut buf)?;
        return Ok(buf);
    }
    let path = last_cache_path()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no cache dir; set FAFIND_LAST"))?;
    read_cache(&path)
}

// faf never writes the last-hits cache file itself, so a directory sitting
// at that path just means no hits have been recorded yet.
fn read_cache(path: &Path) -> io::Result<Vec<u8>> {
    match std::fs::read(path) {
        Err(e) if e.kind() == io::ErrorKind::IsADirectory => {
            Err(io::Error::new(io::ErrorKind::NotFound, "no last faf hits"))
        }
        other => other,
    }
}

fn annotate_directory(e: io::Error, path: &Path) -> io::Error {
    if e.kind() == io::ErrorKind::IsADirectory {
        io::Error::new(
            e.kind(),
            format!("{}: is a directory, not a fafind hits file", path.display()),
        )
    } else {
        e
    }
}

pub fn is_yes(answer: &str) -> bool {
    matches!(answer.trim(), "y" | "Y")
}

fn confirm(count: usize) -> bool {
    eprint!("Are you sure you want to delete ({count}) of files? (N/y) ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    let ok = confirm_read(&mut line);
    ok && is_yes(&line)
}

fn confirm_read(line: &mut String) -> bool {
    #[cfg(unix)]
    {
        if let Ok(tty) = std::fs::File::open("/dev/tty") {
            return io::BufReader::new(tty).read_line(line).is_ok();
        }
    }
    io::stdin().read_line(line).is_ok()
}

pub fn run() {
    let mut file: Option<PathBuf> = None;
    let mut skip_confirm = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                eprint!(
                    "\
delfaf - delete fafind matches

  faf node_modules / | delfaf
  faf node_modules /
  delfaf
  delfaf paths.txt
  delfaf -y paths.txt
"
                );
                std::process::exit(0);
            }
            "-V" | "--version" => {
                eprintln!("delfaf {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-y" | "--yes" => skip_confirm = true,
            _ => file = Some(PathBuf::from(arg)),
        }
    }

    let bytes = match load_input(file.as_deref()) {
        Ok(b) => b,
        Err(e) if file.is_none() && e.kind() == io::ErrorKind::NotFound => {
            eprintln!("delfaf: no last faf hits. pipe faf into delfaf, or run faf first");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("delfaf: {e}");
            std::process::exit(2);
        }
    };

    let paths = parse_paths(&bytes);
    if paths.is_empty() {
        eprintln!("delfaf: no paths");
        std::process::exit(1);
    }

    if !skip_confirm && !confirm(paths.len()) {
        eprintln!("delfaf: aborted");
        std::process::exit(1);
    }

    let start = std::time::Instant::now();
    let (deleted, failed) = delete_paths(&paths);
    let secs = start.elapsed().as_secs_f64();

    eprintln!("delfaf: deleted {deleted} in {secs:.2}s ({failed} failed)");

    std::process::exit(if failed == 0 { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn tmp_dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("delfaf-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn parse_newline_paths() {
        let got = parse_paths(b"/a/node_modules\n/b/node_modules\n");
        assert_eq!(
            got,
            vec![
                PathBuf::from("/a/node_modules"),
                PathBuf::from("/b/node_modules")
            ]
        );
    }

    #[test]
    fn parse_skips_empty_lines() {
        let got = parse_paths(b"/a\n\n/b\n");
        assert_eq!(got, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn parse_strips_ansi() {
        let raw = b"\x1b[2m/app/\x1b[0m\x1b[32mnode_modules\x1b[0m\n";
        let got = parse_paths(raw);
        assert_eq!(got, vec![PathBuf::from("/app/node_modules")]);
    }

    #[test]
    fn parse_nul_separated() {
        let got = parse_paths(b"/a/node_modules\0/b/node_modules\0");
        assert_eq!(
            got,
            vec![
                PathBuf::from("/a/node_modules"),
                PathBuf::from("/b/node_modules")
            ]
        );
    }

    #[test]
    fn forbids_root_and_dot() {
        assert!(is_forbidden(Path::new("/")));
        assert!(is_forbidden(Path::new(".")));
        assert!(is_forbidden(Path::new("..")));
        assert!(!is_forbidden(Path::new("/tmp/node_modules")));
        assert!(is_forbidden(Path::new("/tmp/../etc")));
        assert!(is_forbidden(Path::new("/./")));
        assert!(is_forbidden(Path::new("foo/../../bar")));
    }

    #[test]
    fn deletes_file() {
        let dir = tmp_dir("file");
        let f = dir.join("gone.txt");
        fs::write(&f, b"x").unwrap();
        assert!(delete_path(&f).unwrap());
        assert!(!f.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deletes_directory_tree() {
        let dir = tmp_dir("tree");
        let nm = dir.join("node_modules");
        fs::create_dir_all(nm.join("pkg")).unwrap();
        fs::write(nm.join("pkg/index.js"), b"x").unwrap();
        assert!(delete_path(&nm).unwrap());
        assert!(!nm.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skip_missing_is_ok() {
        let p = PathBuf::from("/tmp/delfaf-does-not-exist-xyz");
        assert!(!delete_path(&p).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn deletes_symlink_not_target() {
        let dir = tmp_dir("sym");
        let target = dir.join("real");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"x").unwrap();
        let link = dir.join("node_modules");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(delete_path(&link).unwrap());
        assert!(link.symlink_metadata().is_err());
        assert!(target.join("keep").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn deletes_dir_symlink_not_target() {
        let dir = tmp_dir("win-sym");
        let target = dir.join("real");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"x").unwrap();
        let link = dir.join("node_modules");
        std::os::windows::fs::symlink_dir(&target, &link).unwrap();
        assert!(delete_path(&link).unwrap());
        assert!(link.symlink_metadata().is_err());
        assert!(target.join("keep").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_slash() {
        assert!(delete_path(Path::new("/")).is_err());
    }

    #[test]
    fn delete_paths_counts() {
        let dir = tmp_dir("many");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"1").unwrap();
        fs::write(&b, b"2").unwrap();
        let (ok, err) = delete_paths(&[a.clone(), b.clone(), dir.join("missing")]);
        assert_eq!(ok, 2);
        assert_eq!(err, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_last_cache<T>(path: &Path, f: impl FnOnce() -> T) -> T {
        let _g = ENV_LOCK.lock().unwrap();
        let prev = std::env::var_os("FAFIND_LAST");
        unsafe {
            std::env::set_var("FAFIND_LAST", path);
        }
        let out = f();
        unsafe {
            match prev {
                Some(v) => std::env::set_var("FAFIND_LAST", v),
                None => std::env::remove_var("FAFIND_LAST"),
            }
        }
        out
    }

    #[test]
    fn last_cache_honors_env() {
        with_last_cache(Path::new("/tmp/custom-faf-last"), || {
            assert_eq!(
                last_cache_path(),
                Some(PathBuf::from("/tmp/custom-faf-last"))
            );
        });
    }

    #[test]
    fn read_last_cache_file() {
        let dir = tmp_dir("cache");
        let cache = dir.join("last");
        fs::write(&cache, b"/z/node_modules\n").unwrap();
        let bytes = with_last_cache(&cache, || {
            std::fs::read(last_cache_path().unwrap()).unwrap()
        });
        assert_eq!(parse_paths(&bytes), vec![PathBuf::from("/z/node_modules")]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_input_reads_file() {
        let dir = tmp_dir("input");
        let list = dir.join("hits.txt");
        let mut f = fs::File::create(&list).unwrap();
        writeln!(f, "/x/node_modules").unwrap();
        let bytes = load_input(Some(&list)).unwrap();
        assert_eq!(parse_paths(&bytes), vec![PathBuf::from("/x/node_modules")]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_input_rejects_directory_arg() {
        let dir = tmp_dir("dirarg");
        let err = load_input(Some(&dir)).unwrap_err();
        assert!(err.to_string().contains("is a directory"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_directory_reads_as_no_last_hits() {
        let dir = tmp_dir("cachedir");
        let err = read_cache(&dir).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn confirm_yes_is_only_y() {
        assert!(is_yes("y"));
        assert!(is_yes("Y"));
        assert!(is_yes(" y\n"));
        assert!(!is_yes(""));
        assert!(!is_yes("\n"));
        assert!(!is_yes("n"));
        assert!(!is_yes("N"));
        assert!(!is_yes("yes"));
        assert!(!is_yes("no"));
    }
}
