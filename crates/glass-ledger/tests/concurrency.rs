use glass_ledger::{BrowserActionWriter, verify_chain};
use std::{io::BufRead, process::Command, sync::{Arc, Barrier}, thread, time::{Duration, Instant}};
use tempfile::tempdir;

fn check(dir: &std::path::Path, count: usize) {
    let path = dir.join("browser.actions.jsonl");
    let result = verify_chain(&path, None);
    assert!(result.ok, "{result:?}");
    assert_eq!(result.count, count);
    let signer = glass_ledger::load_or_create_signer(&dir.join("writer.key")).unwrap();
    let signed = verify_chain(&path, Some(&signer.public_pem));
    assert!(signed.ok, "{signed:?}");
    let lines: Vec<_> = std::io::BufReader::new(std::fs::File::open(path).unwrap()).lines().collect();
    assert_eq!(lines.len(), count);
    for line in lines { serde_json::from_str::<serde_json::Value>(&line.unwrap()).unwrap(); }
}
#[test]
fn concurrent_threads() {
    let dir = tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8).map(|_| {
        let path = dir.path().to_owned(); let barrier = barrier.clone();
        thread::spawn(move || { let w = BrowserActionWriter::new(&path).unwrap(); barrier.wait();
            for _ in 0..200 { w.write_action("navigate", "test", "ALLOW", Default::default()).unwrap(); }
        })
    }).collect();
    for t in threads { t.join().unwrap(); } check(dir.path(), 1600);
}
#[test]
fn concurrent_processes() {
    let dir = tempdir().unwrap();
    let mut children: Vec<_> = (0..6).map(|_| Command::new(std::env::current_exe().unwrap()).args(["--exact", "helper_process"]).env("GLASS_LEDGER_HELPER_DIR", dir.path()).spawn().unwrap()).collect();
    let deadline = Instant::now() + Duration::from_secs(30);
    while std::fs::read_dir(dir.path()).unwrap().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().starts_with("ready-")).count() < 6 {
        assert!(Instant::now() < deadline); thread::sleep(Duration::from_millis(5));
    }
    std::fs::write(dir.path().join("start"), "").unwrap();
    for child in &mut children { assert!(child.wait().unwrap().success()); } check(dir.path(), 1200);
}
#[test]
fn damaged_tail_rejected_without_changes() {
    let dir = tempdir().unwrap(); let w = BrowserActionWriter::new(dir.path()).unwrap();
    w.write_action("navigate", "test", "ALLOW", Default::default()).unwrap();
    let path = dir.path().join("browser.actions.jsonl");
    use std::io::Write;
    write!(std::fs::OpenOptions::new().append(true).open(&path).unwrap(), "{{\"sig\":").unwrap();
    let before = std::fs::read(&path).unwrap();
    let err = w.write_action("navigate", "test", "ALLOW", Default::default()).unwrap_err();
    assert!(err.to_string().contains("last record is damaged")); assert!(err.to_string().contains(path.to_str().unwrap()));
    assert_eq!(before, std::fs::read(path).unwrap());
}
#[test]
fn append_cost_does_not_scale_with_history() {
    let dir = tempdir().unwrap(); let w = BrowserActionWriter::new(dir.path()).unwrap();
    w.write_action("navigate", "test", "ALLOW", Default::default()).unwrap();
    let path = dir.path().join("browser.actions.jsonl"); let mut line = std::fs::read(&path).unwrap();
    line.pop(); line.extend(std::iter::repeat(b' ').take(4096)); line.push(b'\n');
    let measure = |n: usize| {
        use std::io::Write;
        let mut f = std::fs::File::create(&path).unwrap(); for _ in 0..n { f.write_all(&line).unwrap(); } drop(f);
        let start = Instant::now(); for _ in 0..100 { w.write_action("navigate", "test", "ALLOW", Default::default()).unwrap(); } start.elapsed()
    };
    let small = measure(200); let large = measure(20_000);
    assert!(large < small * 8, "small={small:?}, large={large:?}");
}

#[test]
fn helper_process() {
    let Some(dir) = std::env::var_os("GLASS_LEDGER_HELPER_DIR") else { return; };
    let dir = std::path::Path::new(&dir);
    let writer = BrowserActionWriter::new(dir).unwrap();
    std::fs::write(dir.join(format!("ready-{}", std::process::id())), "").unwrap();
    while !dir.join("start").exists() { thread::sleep(Duration::from_millis(1)); }
    for _ in 0..200 { writer.write_action("navigate", "test", "ALLOW", Default::default()).unwrap(); }
}

#[test]
fn held_lock_times_out_then_recovers() {
    struct Holder(std::process::Child);
    impl Drop for Holder { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    let dir = tempdir().unwrap();
    let writer = BrowserActionWriter::new(dir.path()).unwrap();
    std::env::set_var("OMNIS_GLASS_LEDGER_LOCK_TIMEOUT_MS", "250");
    let mut holder = Holder(Command::new(std::env::current_exe().unwrap()).args(["--exact", "held_lock_helper"])
        .env("GLASS_HELD_LEDGER_DIR", dir.path()).spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(30);
    while !dir.path().join("held-ready").exists() { assert!(Instant::now() < deadline); thread::sleep(Duration::from_millis(5)); }
    let path = dir.path().join("browser.actions.jsonl");
    for initialize in [true, false] {
        let start = Instant::now();
        let result = if initialize { BrowserActionWriter::new(dir.path()).map(|_| ()) }
            else { writer.write_action("navigate", "test", "ALLOW", Default::default()).map(|_| ()) };
        let error = result.unwrap_err(); let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(200) && elapsed < Duration::from_secs(2), "{elapsed:?}");
        assert!(error.to_string().contains(path.to_str().unwrap()), "{error}");
        assert!(error.to_string().contains("locked by another process"), "{error}");
    }
    std::env::remove_var("OMNIS_GLASS_LEDGER_LOCK_TIMEOUT_MS");
    let start = Instant::now();
    let error = writer.write_action("navigate", "test", "ALLOW", Default::default()).unwrap_err();
    assert!(start.elapsed() >= Duration::from_millis(1_900) && start.elapsed() < Duration::from_secs(4), "{:?}", start.elapsed());
    assert!(error.to_string().contains("locked by another process"), "{error}");
    std::fs::write(dir.path().join("held-release"), "").unwrap();
    assert!(holder.0.wait().unwrap().success());
    writer.write_action("navigate", "test", "ALLOW", Default::default()).unwrap();
}

#[test]
fn held_lock_helper() {
    let Some(dir) = std::env::var_os("GLASS_HELD_LEDGER_DIR") else { return; };
    let dir = std::path::Path::new(&dir);
    let file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("browser.actions.jsonl")).unwrap();
    file.lock().unwrap();
    std::fs::write(dir.join("held-ready"), "").unwrap();
    while !dir.join("held-release").exists() { thread::sleep(Duration::from_millis(5)); }
}
