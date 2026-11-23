//! Clone tests against a local repository created on the fly (no network needed).

use std::path::{Path, PathBuf};
use std::process::Command;

use mxkeys_git::{clone, detect_backend, parse_repo_url, Backend, CloneProgress, GitError};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mxkeys-git-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// A tiny zmk-config-like repository.
fn source_repo(name: &str) -> PathBuf {
    let src = tmp(name);
    std::fs::create_dir_all(src.join("config")).unwrap();
    std::fs::write(
        src.join("build.yaml"),
        "include:\n  - board: nice_nano@2.0.0\n    shield: corne_left\n",
    )
    .unwrap();
    std::fs::write(
        src.join("config/corne.keymap"),
        "/ { keymap { compatible = \"zmk,keymap\"; }; };\n",
    )
    .unwrap();
    git(&src, &["init", "-q"]);
    git(&src, &["add", "."]);
    git(&src, &["commit", "-q", "-m", "init"]);
    src
}

fn run(backend: &Backend, tag: &str) {
    let src = source_repo(&format!("src-{tag}"));
    let dest = tmp(&format!("dest-{tag}"));
    let url = parse_repo_url(src.to_str().unwrap()).unwrap();
    let mut events = Vec::new();
    clone(&url, &dest, backend, |p| events.push(p)).unwrap();
    assert!(dest.join("config/corne.keymap").is_file());
    assert!(dest.join(".git").exists());
    assert_eq!(events.first(), Some(&CloneProgress::Connecting));
    assert_eq!(events.last(), Some(&CloneProgress::Done));

    // Cloning again into the now non-empty folder is refused with a friendly error.
    let again = clone(&url, &dest, backend, |_| {});
    assert!(
        matches!(again, Err(GitError::DestinationNotEmpty(_))),
        "{again:?}"
    );

    // A missing source is reported, not panicked on.
    let missing = parse_repo_url(src.join("nope").to_str().unwrap()).unwrap();
    assert!(clone(&missing, &tmp(&format!("missing-{tag}")), backend, |_| {}).is_err());

    std::fs::remove_dir_all(&src).ok();
    std::fs::remove_dir_all(&dest).ok();
}

#[test]
fn clone_with_system_git() {
    let backend = detect_backend();
    assert!(
        matches!(backend, Backend::SystemGit { .. }),
        "CI runners have git installed"
    );
    run(&backend, "system");
}

#[test]
fn clone_with_libgit2() {
    run(&Backend::Libgit2, "libgit2");
}

#[test]
fn ssh_without_system_git_is_explained() {
    let url = parse_repo_url("git@example.invalid:me/zmk-config.git").unwrap();
    let r = clone(&url, &tmp("ssh"), &Backend::Libgit2, |_| {});
    assert!(matches!(r, Err(GitError::SshNeedsSystemGit)));
}

/// Real HTTPS clone of the owner's public config. Network-dependent, so opt-in:
/// `cargo test -p mxkeys-git -- --ignored`.
#[test]
#[ignore]
fn clone_public_https_both_backends() {
    for (tag, backend) in [
        ("net-system", detect_backend()),
        ("net-libgit2", Backend::Libgit2),
    ] {
        let dest = tmp(tag);
        let url = parse_repo_url("https://github.com/xnmrph/corne").unwrap();
        let mut events = Vec::new();
        clone(&url, &dest, &backend, |p| events.push(p)).unwrap();
        assert!(dest.join("config/corne.keymap").is_file(), "{tag}");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, CloneProgress::Receiving { .. })),
            "{tag}: {events:?}"
        );
        std::fs::remove_dir_all(&dest).ok();
    }
}
