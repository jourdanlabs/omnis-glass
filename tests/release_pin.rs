use std::path::Path;
use std::process::Command;

pub fn check(root: &Path, dir: &Path, binary: &str, key: &str) {
    let bin = dir.join(binary);
    assert!(bin.is_file(), "release binary missing: {}", bin.display());
    let stamp = std::fs::read_to_string(dir.join("GLASS_BUILD_HEAD"))
        .expect("release stamp missing or unreadable");
    let stamp = stamp.trim();
    assert!(!stamp.ends_with("-dirty"), "release stamp is dirty");
    let git = |args: &[&str]| {
        let out = Command::new("git").args(args).current_dir(root).output().expect("run git");
        assert!(out.status.success(), "git command failed: {:?}", args);
        String::from_utf8(out.stdout).expect("git output UTF-8")
    };
    let head = git(&["rev-parse", "HEAD"]);
    let head = head.trim();
    assert_eq!(stamp, head, "release stamp differs from HEAD");
    let changes = git(&["status", "--porcelain", "--untracked-files=normal"]);
    assert!(changes.is_empty(), "working tree has uncommitted changes:\n{changes}");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("GLASS_RELEASE_MANIFEST.json"))
        .expect("release manifest missing or unreadable")).expect("invalid release manifest");
    assert_eq!(manifest["git_head"].as_str(), Some(head), "manifest git head differs from HEAD");
    assert_eq!(manifest["dirty"].as_bool(), Some(false), "manifest dirty flag is missing or true");
    let digest = Command::new("shasum").args(["-a", "256"]).arg(&bin).output().expect("run shasum");
    assert!(digest.status.success(), "sha256 failed for {binary}");
    let hash = String::from_utf8(digest.stdout).expect("sha256 output UTF-8");
    assert_eq!(manifest[key].as_str(), hash.split_whitespace().next(), "binary sha256 differs from manifest: {binary}");
    let omnis = dir.join("omnis-glass");
    assert!(omnis.is_file(), "release binary missing: {}", omnis.display());
    let omnis_hash = Command::new("shasum").args(["-a", "256"]).arg(&omnis).output().expect("run shasum");
    assert!(omnis_hash.status.success(), "sha256 failed for omnis-glass");
    let omnis_hash = String::from_utf8(omnis_hash.stdout).expect("sha256 output UTF-8");
    assert_eq!(manifest["omnis_glass"].as_str(), omnis_hash.split_whitespace().next(), "binary sha256 differs from manifest: omnis-glass");
    let output = Command::new(&omnis).arg("--version").output().expect("run omnis-glass --version");
    assert!(output.status.success(), "omnis-glass --version failed");
    let version = String::from_utf8_lossy(&output.stdout);
    assert!(version.contains(&format!("({head}, clean)")), "omnis-glass --version reports wrong commit or dirty build: {version}");
}
