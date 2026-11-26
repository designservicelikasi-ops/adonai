//! Architecture rules that the compiler cannot express, checked on the source tree.

use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if p.is_dir() {
            sources(&p, out);
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Files (relative to `crates/`) containing `needle`, outside tests.
fn users(needle: &str) -> Vec<String> {
    let mut files = Vec::new();
    sources(&crates_dir(), &mut files);
    let mut v: Vec<String> = files
        .into_iter()
        .filter(|p| !p.components().any(|c| c.as_os_str() == "tests") && p.file_name().is_some_and(|n| n != "tests.rs"))
        .filter(|p| std::fs::read_to_string(p).unwrap_or_default().contains(needle))
        .map(|p| p.strip_prefix(crates_dir()).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    v.sort();
    v
}

#[test]
fn only_the_user_command_path_can_confirm_a_grant() {
    // No engine, model, brain, shadow or AI module can build the proof of a human request.
    assert_eq!(users("UserConfirmation::from_user_command"), vec!["aegis-core/src/autonomy.rs"]);
    assert_eq!(users("fn from_user_command"), vec!["aegis-autonomy/src/lib.rs"]);
    // ...and the engine helper that uses it is only called by the user command handler.
    assert_eq!(users(".grant_autonomy("), vec!["aegis-core/src/command.rs"]);
}

#[test]
fn shadow_and_strategies_cannot_reach_execution_or_permissions() {
    for (krate, forbidden) in [
        ("aegis-shadow", ["aegis-execution", "aegis-autonomy", "aegis-protocol"]),
        ("aegis-strategy", ["aegis-execution", "aegis-autonomy", "aegis-risk"]),
        ("aegis-brain", ["aegis-execution", "aegis-autonomy", "aegis-protocol"]),
    ] {
        let manifest = std::fs::read_to_string(crates_dir().join(krate).join("Cargo.toml")).unwrap();
        // Dependency lines only (comments explain the absence of some of them).
        let deps: Vec<&str> = manifest.lines().map(str::trim).filter(|l| !l.starts_with('#')).collect();
        for f in forbidden {
            assert!(!deps.iter().any(|l| l.starts_with(f)), "{krate} must not depend on {f}");
        }
    }
}
