use glass_capture::{WebCaptureWriter,ScreencastFrameWriter,build_capture,CaptureKind,CaptureSource};
use std::{io::{BufRead,Write},process::Command,sync::{Arc,Barrier},thread,time::{Duration,Instant}};
fn dir() -> std::path::PathBuf { let p = std::env::temp_dir().join(format!("capture-concurrency-{}-{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap())); std::fs::create_dir_all(&p).unwrap(); p }
fn check(p: &std::path::Path, n: usize) {
 for name in ["web.captures.jsonl", "screencast-frames.jsonl"] {
  let lines: Vec<_> = std::io::BufReader::new(std::fs::File::open(p.join(name)).unwrap()).lines().collect();
  let records: Vec<serde_json::Value> = lines.into_iter().map(|line| serde_json::from_str(&line.unwrap()).unwrap()).collect();
  assert_eq!(records.iter().filter(|v| v.get("probe").is_none()).count(), n, "{name}");
  assert_eq!(records.iter().filter(|v| v.get("probe").is_some()).count(), 1, "{name}");
 }
}
// A cooperating writer holds each advisory lock while its record is incomplete.
fn hold_partial_records(p: &std::path::Path) -> Vec<std::fs::File> {
 ["web.captures.jsonl", "screencast-frames.jsonl"].iter().map(|name| {
  let mut file=std::fs::OpenOptions::new().create(true).append(true).open(p.join(name)).unwrap();
  file.lock().unwrap(); file.write_all(b"{\"probe\":").unwrap(); file
 }).collect()
}
fn finish_partial_records(mut files: Vec<std::fs::File>) {
 thread::sleep(Duration::from_millis(100));
 let mut web = files.remove(0);
 web.write_all(b"true}\n").unwrap(); web.flush().unwrap(); drop(web);
 thread::sleep(Duration::from_millis(100));
 let mut frames = files.remove(0);
 frames.write_all(b"true}\n").unwrap(); frames.flush().unwrap();
}
#[test]
fn concurrent_threads() {
 let p = dir(); let held=hold_partial_records(&p); let barrier = Arc::new(Barrier::new(8));
 let threads: Vec<_> = (0..8).map(|_| { let p=p.clone(); let barrier=barrier.clone(); thread::spawn(move || { let web=WebCaptureWriter::new(&p).unwrap(); let frames=ScreencastFrameWriter::new(&p).unwrap(); let rec=build_capture("https://example.com","https://example.com",b"data",CaptureSource::PageContent,CaptureKind::Fixture,None,None).unwrap(); barrier.wait(); for _ in 0..200 { web.write(&rec).unwrap(); frames.write(&"a".repeat(64),"session").unwrap(); } }) }).collect();
 finish_partial_records(held); for t in threads { t.join().unwrap(); } check(&p,1600); std::fs::remove_dir_all(p).unwrap();
}
#[test]
fn concurrent_processes() {
 let p=dir(); let held=hold_partial_records(&p); let mut children: Vec<_>=(0..6).map(|_| Command::new(std::env::current_exe().unwrap()).args(["--exact", "helper_process"]).env("GLASS_CAPTURE_HELPER_DIR", &p).spawn().unwrap()).collect(); let deadline=Instant::now()+Duration::from_secs(30);
 while std::fs::read_dir(&p).unwrap().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().starts_with("ready-")).count()<6 { assert!(Instant::now()<deadline); thread::sleep(Duration::from_millis(5)); }
 std::fs::write(p.join("start"),"").unwrap(); finish_partial_records(held); for c in &mut children { assert!(c.wait().unwrap().success()); } check(&p,1200); std::fs::remove_dir_all(p).unwrap();
}

#[test]
fn helper_process() {
 let Some(dir) = std::env::var_os("GLASS_CAPTURE_HELPER_DIR") else { return; };
 let dir = std::path::Path::new(&dir);
 let web=WebCaptureWriter::new(dir).unwrap(); let frames=ScreencastFrameWriter::new(dir).unwrap();
 let rec=build_capture("https://example.com", "https://example.com", b"data", CaptureSource::PageContent, CaptureKind::Fixture, None, None).unwrap();
 std::fs::write(dir.join(format!("ready-{}", std::process::id())), "").unwrap();
 while !dir.join("start").exists() { thread::sleep(Duration::from_millis(1)); }
 for _ in 0..200 { web.write(&rec).unwrap(); frames.write(&"a".repeat(64), "session").unwrap(); }
}

#[test]
fn held_capture_locks_time_out_then_recover() {
 struct Holder(std::process::Child); impl Drop for Holder { fn drop(&mut self) { let _=self.0.kill(); let _=self.0.wait(); } }
 let p=dir(); std::env::set_var("OMNIS_GLASS_LEDGER_LOCK_TIMEOUT_MS", "250");
 let mut holder=Holder(Command::new(std::env::current_exe().unwrap()).args(["--exact", "held_capture_lock_helper"]).env("GLASS_HELD_CAPTURE_DIR", &p).spawn().unwrap());
 let deadline=Instant::now()+Duration::from_secs(30);
 while !p.join("held-ready").exists() { assert!(Instant::now()<deadline); thread::sleep(Duration::from_millis(5)); }
 let web=WebCaptureWriter::new(&p).unwrap(); let frames=ScreencastFrameWriter::new(&p).unwrap();
 let rec=build_capture("https://example.com", "https://example.com", b"data", CaptureSource::PageContent, CaptureKind::Fixture, None, None).unwrap();
 for name in ["web.captures.jsonl", "screencast-frames.jsonl"] {
  let start=Instant::now(); let result=if name.starts_with("web") { web.write(&rec) } else { frames.write(&"a".repeat(64), "session") };
  let error=result.unwrap_err(); let elapsed=start.elapsed();
  assert!(elapsed>=Duration::from_millis(200) && elapsed<Duration::from_secs(2), "{elapsed:?}");
  assert!(error.to_string().contains(p.join(name).to_str().unwrap()), "{error}");
  assert!(error.to_string().contains("locked by another process"), "{error}");
 }
 std::fs::write(p.join("held-release"), "").unwrap(); assert!(holder.0.wait().unwrap().success());
 web.write(&rec).unwrap(); frames.write(&"a".repeat(64), "session").unwrap(); std::fs::remove_dir_all(p).unwrap();
}

#[test]
fn held_capture_lock_helper() {
 let Some(dir)=std::env::var_os("GLASS_HELD_CAPTURE_DIR") else { return; }; let p=std::path::Path::new(&dir);
 let files: Vec<_>=["web.captures.jsonl", "screencast-frames.jsonl"].iter().map(|name| {
  let file=std::fs::OpenOptions::new().create(true).append(true).open(p.join(name)).unwrap(); file.lock().unwrap(); file
 }).collect();
 std::fs::write(p.join("held-ready"), "").unwrap();
 while !p.join("held-release").exists() { thread::sleep(Duration::from_millis(5)); }
 drop(files);
}
