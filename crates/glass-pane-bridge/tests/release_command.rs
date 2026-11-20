#![cfg(unix)]
use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};

fn fixture(mode: &str) {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo"); let scripts = repo.join("scripts"); let bin = temp.path().join("bin"); let target = temp.path().join("target");
    std::fs::create_dir_all(&scripts).unwrap(); std::fs::create_dir_all(&bin).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in ["build-release.sh", "stamp-glass-release.sh"] {
        let path = scripts.join(name); std::fs::copy(root.join("scripts").join(name), &path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let fake_cargo = bin.join("cargo");
    std::fs::write(&fake_cargo, r##"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FAKE_LOG"
mkdir -p "$CARGO_TARGET_DIR/release"
head=$(git rev-parse HEAD)
case "$*" in
  'clean --release --workspace') ;;
  'build --release -p obscura-cli --bins --features render') printf '#!/bin/sh\necho omnis-before-list\n' > "$CARGO_TARGET_DIR/release/omnis-glass" ;;
  'build --release -p glass-pane --features render') printf '#!/bin/sh\necho "glass-pane 0.1.0 (%s, clean)"\n' "$head" > "$CARGO_TARGET_DIR/release/glass-pane" ;;
  'nextest list --release --features render')
    case "$FAKE_MODE" in wrong) head=0000000000000000000000000000000000000000; word=clean ;; dirty) word=dirty ;; *) word=clean ;; esac
    printf '#!/bin/sh\necho "omnis-glass 0.1.0 (%s, %s)"\n' "$head" "$word" > "$CARGO_TARGET_DIR/release/omnis-glass" ;;
  *) echo "unexpected cargo command: $*" >&2; exit 1 ;;
esac
chmod +x "$CARGO_TARGET_DIR/release/omnis-glass" 2>/dev/null || true
if [ -e "$CARGO_TARGET_DIR/release/glass-pane" ]; then chmod +x "$CARGO_TARGET_DIR/release/glass-pane"; fi
"##).unwrap();
    std::fs::set_permissions(&fake_cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    let git = |args: &[&str]| { let output = Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap(); assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr)); String::from_utf8(output.stdout).unwrap() };
    git(&["init", "-q"]); git(&["add", "scripts"]); git(&["-c", "user.name=Release Test", "-c", "user.email=release-test@example.test", "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]);
    let release = target.join("release"); std::fs::create_dir_all(&release).unwrap(); std::fs::write(release.join("GLASS_RELEASE_MANIFEST.json"), "old manifest").unwrap(); std::fs::write(release.join("GLASS_BUILD_HEAD"), "old head").unwrap();
    let log = temp.path().join("calls"); let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let output = Command::new(scripts.join("build-release.sh")).env("PATH", path).env("FAKE_LOG", &log).env("FAKE_MODE", mode)
        .env("CARGO_TARGET_DIR", &target).env("GLASS_RELEASE_TARGET", &release).output().unwrap();
    let calls = std::fs::read_to_string(log).unwrap();
    assert_eq!(calls.lines().collect::<Vec<_>>(), vec!["clean --release --workspace", "build --release -p obscura-cli --bins --features render", "build --release -p glass-pane --features render", "nextest list --release --features render"]);
    if mode != "ok" {
        assert!(!output.status.success(), "{mode} should be refused");
        assert!(!release.join("GLASS_RELEASE_MANIFEST.json").exists(), "{mode} left a manifest");
        assert!(!release.join("GLASS_BUILD_HEAD").exists(), "{mode} left a head stamp");
        let stderr = String::from_utf8_lossy(&output.stderr); assert!(stderr.contains("release version mismatch") && stderr.contains(git(&["rev-parse", "HEAD"]).trim()), "{stderr}");
        return;
    }
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap(); assert!(stdout.contains("omnis-glass 0.1.0"), "{stdout}"); assert!(stdout.contains("glass-pane 0.1.0"), "{stdout}");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(release.join("GLASS_RELEASE_MANIFEST.json")).unwrap()).unwrap();
    let hash = |name: &str| { let result = Command::new("shasum").args(["-a", "256"]).arg(release.join(name)).output().unwrap(); assert!(result.status.success()); String::from_utf8(result.stdout).unwrap().split_whitespace().next().unwrap().to_string() };
    assert_eq!(manifest["omnis_glass"], hash("omnis-glass")); assert_eq!(manifest["glass_pane"], hash("glass-pane"));
    assert_eq!(manifest["git_head"].as_str().unwrap(), git(&["rev-parse", "HEAD"]).trim()); assert_eq!(manifest["dirty"], false);
}

#[test] fn release_command_stamps_the_post_enumeration_binaries() { fixture("ok"); }
#[test] fn release_command_refuses_wrong_commit() { fixture("wrong"); }
#[test] fn release_command_refuses_dirty_binary() { fixture("dirty"); }

#[test]
fn build_info_refreshes_across_checkouts_sharing_target() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("base"); let first = temp.path().join("first"); let second = temp.path().join("second"); let target = temp.path().join("target");
    let package = base.join("crates/glass-build-info");
    std::fs::create_dir_all(package.join("src")).unwrap();
    std::fs::create_dir_all(base.join(".cargo")).unwrap();
    std::fs::write(base.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/glass-build-info\"]\nresolver = \"2\"\n").unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::copy(root.join(".cargo/config.toml"), base.join(".cargo/config.toml")).unwrap();
    std::fs::copy(root.join("crates/glass-build-info/build.rs"), package.join("build.rs")).unwrap();
    std::fs::copy(root.join("crates/glass-build-info/src/lib.rs"), package.join("src/lib.rs")).unwrap();
    std::fs::write(package.join("Cargo.toml"), "[package]\nname = \"glass-build-info\"\nversion = \"0.1.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n").unwrap();
    std::fs::write(package.join("src/main.rs"), "fn main() { println!(\"{}\", glass_build_info::version_line(\"fixture\")); }\n").unwrap();
    let lock = Command::new("cargo").current_dir(&base).args(["generate-lockfile", "--offline"]).env_remove("CARGO_TARGET_DIR").output().unwrap();
    assert!(lock.status.success(), "{}", String::from_utf8_lossy(&lock.stderr));
    let git = |repo: &Path, args: &[&str]| -> String {
        let output = Command::new("git").arg("-C").arg(repo).args(args).output().unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&base, &["init", "-q"]); git(&base, &["add", "."]);
    git(&base, &["-c", "user.name=Release Test", "-c", "user.email=release-test@example.test", "-c", "commit.gpgsign=false", "commit", "-qm", "first"]);
    git(&base, &["pack-refs", "--all", "--no-prune"]);
    git(&base, &["worktree", "add", "--detach", first.to_str().unwrap(), "HEAD"]);
    let first_head = git(&first, &["rev-parse", "HEAD"]);
    git(&base, &["worktree", "add", "--detach", second.to_str().unwrap(), "HEAD"]);
    git(&second, &["-c", "user.name=Release Test", "-c", "user.email=release-test@example.test", "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", "second"]);
    let second_head = git(&second, &["rev-parse", "HEAD"]); assert_ne!(first_head, second_head);
    assert!(git(&first, &["status", "--porcelain"]).is_empty());
    assert!(git(&second, &["status", "--porcelain"]).is_empty());
    let build = |repo: &Path| -> String {
        let output = Command::new("cargo").current_dir(repo).args(["run", "--offline", "--quiet", "--target-dir"]).arg(&target)
            .env_remove("CARGO_TARGET_DIR").env_remove("GLASS_BUILD_ROOT").env("GIT_OPTIONAL_LOCKS", "0").output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap()
    };
    let first_version = build(&first); assert!(first_version.contains(&format!("({first_head}, clean)")), "{first_version}");
    let binary = target.join("debug/glass-build-info"); let unchanged = std::fs::metadata(&binary).unwrap().modified().unwrap();
    assert_eq!(build(&first), first_version); assert_eq!(std::fs::metadata(&binary).unwrap().modified().unwrap(), unchanged, "unchanged build metadata must not relink");
    let second_version = build(&second); assert!(second_version.contains(&format!("({second_head}, clean)")), "{second_version}");
}

#[test]
fn release_clean_rebuilds_shared_target_from_checkout() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("repo"); let old = temp.path().join("old");
    let new = temp.path().join("new"); let target = temp.path().join("target");
    std::fs::create_dir_all(base.join("lib/src")).unwrap();
    std::fs::create_dir_all(base.join("app/src")).unwrap();
    for (path, text) in [
        ("Cargo.toml", "[workspace]\nmembers=[\"lib\",\"app\"]\nresolver=\"2\"\n"),
        ("lib/Cargo.toml", "[package]\nname=\"s13-lib\"\nversion=\"0.1.0\"\nedition=\"2021\"\n"),
        ("app/Cargo.toml", "[package]\nname=\"s13-app\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\ns13-lib={path=\"../lib\"}\n"),
        ("lib/src/lib.rs", "pub fn value()->&'static str {\"old\"}\n"),
        ("app/src/main.rs", "fn main(){println!(\"{}\",s13_lib::value());}\n"),
    ] { std::fs::write(base.join(path), text).unwrap(); }
    let run = |dir: &Path, program: &str, args: &[&str]| -> String {
        let output = Command::new(program).current_dir(dir).args(args)
            .env("CARGO_TARGET_DIR", &target).env("CARGO_INCREMENTAL", "0")
            .env_remove("GLASS_BUILD_ROOT").output().unwrap();
        assert!(output.status.success(), "{program} {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    run(&base,"git",&["init","-q"]); run(&base,"git",&["add","."]);
    let commit = ["-c","user.name=Release Test","-c","user.email=release-test@example.test","-c","commit.gpgsign=false","commit","-qm"];
    let mut args = commit.to_vec(); args.push("old"); run(&base,"git",&args);
    run(&base,"git",&["worktree","add","-q","--detach",old.to_str().unwrap(),"HEAD"]);
    std::fs::write(base.join("lib/src/lib.rs"), "pub fn value()->&'static str {\"new\"}\n").unwrap();
    run(&base,"git",&["add","."]); let mut args = commit.to_vec(); args.push("new"); run(&base,"git",&args);
    run(&base,"git",&["worktree","add","-q","--detach",new.to_str().unwrap(),"HEAD"]);
    let build = |dir: &Path| { run(dir,"cargo",&["build","--release","--offline","-p","s13-app"]); };
    let output = || run(&base,target.join("release/s13-app").to_str().unwrap(),&[]);
    build(&new); assert_eq!(output(),"new");
    for path in ["Cargo.toml","lib/Cargo.toml","app/Cargo.toml","lib/src/lib.rs","app/src/main.rs"] {
        run(&old,"touch",&["-t","200001010000",path]);
    }
    build(&old); assert_eq!(output(),"new", "fixture did not reproduce stale workspace artifacts");
    let script = include_str!("../../../scripts/build-release.sh");
    let clean = script.lines().find(|line| line.trim() == "cargo clean --release --workspace")
        .expect("release script must clean workspace release artifacts before building");
    run(&old,"sh",&["-c",clean]);
    build(&old); assert_eq!(output(),"old", "release clean must build the checkout's library");
}
