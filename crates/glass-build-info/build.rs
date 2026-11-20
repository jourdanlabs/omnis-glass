use std::process::Command;

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("run git");
    assert!(output.status.success(), "git {:?} failed", args);
    String::from_utf8(output.stdout).expect("git output is UTF-8").trim().to_owned()
}

fn main() {
    println!("cargo:rerun-if-env-changed=GLASS_BUILD_ROOT");
    let root = git(&["rev-parse", "--show-toplevel"]);
    for path in ["HEAD", "index", "packed-refs"] {
        let resolved = git(&["rev-parse", "--git-path", path]);
        println!("cargo:rerun-if-changed={}", std::path::Path::new(&root).join(resolved).display());
    }
    let symbolic = Command::new("git").args(["symbolic-ref", "-q", "HEAD"]).output().expect("git symbolic-ref");
    if symbolic.status.success() {
        let head_ref = String::from_utf8(symbolic.stdout).expect("git ref UTF-8");
        let reference = git(&["rev-parse", "--git-path", head_ref.trim()]);
        println!("cargo:rerun-if-changed={}", std::path::Path::new(&root).join(reference).display());
    }
    // Track the worktree, including new paths, so a dirty build cannot reuse clean metadata.
    println!("cargo:rerun-if-changed={root}");
    let commit = git(&["rev-parse", "HEAD"]);
    let dirty = !git(&["status", "--porcelain", "--untracked-files=normal"]).is_empty();
    println!("cargo:rustc-env=GLASS_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=GLASS_BUILD_DIRTY={}", if dirty { "dirty" } else { "clean" });
}
