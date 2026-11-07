//! Repository checks for optobench, run in CI with `cargo xtask check`.
//!
//! - `layers`: a crate depends only on crates in its own or a lower layer (docs/design.md, Architecture).
//! - `naming`: no name matches the deny-list in `xtask/naming-denylist.txt`
//!   (docs/design.md, File naming and versioning).
//! - `assets`: every file under `assets/` is listed in `assets/manifest.toml` with an
//!   allowed licence (docs/design.md, Ground rules).
//!
//! Std only, so the checks run before any dependency is fetched.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let task = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "check".to_owned());
    let root = workspace_root();
    let mut problems = Vec::new();
    match task.as_str() {
        "check" => {
            check_layers(&root, &mut problems);
            check_naming(&root, &mut problems);
            check_assets(&root, &mut problems);
        }
        "layers" => check_layers(&root, &mut problems),
        "naming" => check_naming(&root, &mut problems),
        "assets" => check_assets(&root, &mut problems),
        other => {
            eprintln!("unknown task `{other}`; tasks: check, layers, naming, assets");
            return ExitCode::FAILURE;
        }
    }
    if problems.is_empty() {
        println!("xtask {task}: ok");
        ExitCode::SUCCESS
    } else {
        for p in &problems {
            eprintln!("error: {p}");
        }
        eprintln!("xtask {task}: {} problem(s)", problems.len());
        ExitCode::FAILURE
    }
}

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(manifest_dir)
}

/// Crate manifests under `crates/` and `apps/`: (crate name, path to Cargo.toml).
fn crate_manifests(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for group in ["crates", "apps"] {
        let Ok(entries) = fs::read_dir(root.join(group)) else {
            continue;
        };
        for entry in entries.flatten() {
            let manifest = entry.path().join("Cargo.toml");
            if manifest.is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                out.push((name, manifest));
            }
        }
    }
    out.sort();
    out
}

fn layer_rank(layer: &str) -> Option<u32> {
    match layer {
        "apps" => Some(100),
        l => l.strip_prefix('L').and_then(|n| n.parse().ok()),
    }
}

/// Reads `layer = "Lx"` from `[package.metadata.optobench]` and the workspace
/// crates listed under `[dependencies]`. Deliberately simple line parsing.
fn read_manifest(path: &Path) -> (Option<String>, Vec<String>) {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut section = String::new();
    let mut layer = None;
    let mut deps = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.to_owned();
            continue;
        }
        if section == "[package.metadata.optobench]" {
            if let Some(v) = line.strip_prefix("layer") {
                layer = Some(
                    v.trim_start_matches([' ', '='])
                        .trim_matches('"')
                        .to_owned(),
                );
            }
        } else if section == "[dependencies]" {
            if let Some(name) = line.split(['.', ' ', '=']).next() {
                if name.starts_with("optobench-") {
                    deps.push(name.to_owned());
                }
            }
        }
    }
    (layer, deps)
}

fn check_layers(root: &Path, problems: &mut Vec<String>) {
    let manifests = crate_manifests(root);
    let mut layers: BTreeMap<String, u32> = BTreeMap::new();
    let mut all_deps = Vec::new();
    for (name, path) in &manifests {
        let (layer, deps) = read_manifest(path);
        match layer.as_deref().and_then(layer_rank) {
            Some(rank) => {
                layers.insert(name.clone(), rank);
            }
            None => problems.push(format!(
                "{name}: missing or invalid layer in [package.metadata.optobench]"
            )),
        }
        all_deps.push((name.clone(), deps));
    }
    for (name, deps) in all_deps {
        let Some(&own) = layers.get(&name) else {
            continue;
        };
        for dep in deps {
            match layers.get(&dep) {
                Some(&rank) if rank <= own => {}
                Some(_) => problems.push(format!(
                    "{name} depends on {dep}, which is in a higher layer"
                )),
                None => problems.push(format!("{name} depends on unknown workspace crate {dep}")),
            }
        }
    }
}

fn denylist(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join("xtask/naming-denylist.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Returns the deny-listed word a name contains, if any.
fn offending_word<'a>(name: &str, words: &'a [String]) -> Option<&'a str> {
    let lower = name.to_ascii_lowercase();
    let segments: Vec<&str> = lower
        .split(['.', '-', '_', '/', ' '])
        .filter(|s| !s.is_empty())
        .collect();
    words
        .iter()
        .map(String::as_str)
        .find(|w| segments.contains(w) || (w.len() >= 5 && lower.contains(w)))
}

/// Names checked: crate names, binary names, the file extension and kind names
/// declared in optobench-io, command ids in the registry, and every file and directory
/// name in the repository.
fn check_naming(root: &Path, problems: &mut Vec<String>) {
    let words = denylist(root);
    if words.is_empty() {
        problems.push("xtask/naming-denylist.txt is missing or empty".to_owned());
        return;
    }
    let mut names: Vec<String> = crate_manifests(root).into_iter().map(|(n, _)| n).collect();
    names.extend([
        "optobench".to_owned(),
        "sidecar".to_owned(),
        "layout".to_owned(),
        "part".to_owned(),
    ]);
    let ids = command_ids(root);
    if ids.is_empty() {
        problems.push("no command ids found in crates/optobench-commands/src".to_owned());
    }
    names.extend(ids);
    collect_paths(root, root, &mut names);
    for name in names {
        if let Some(word) = offending_word(&name, &words) {
            problems.push(format!("name `{name}` matches deny-listed word `{word}`"));
        }
    }
}

/// Command ids, read from `id: "..."` lines in the registry crate's sources.
fn command_ids(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root.join("crates/optobench-commands/src")) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for entry in entries.flatten() {
        let text = fs::read_to_string(entry.path()).unwrap_or_default();
        for line in text.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("id: \"") {
                if let Some(id) = rest.split('"').next() {
                    ids.push(id.to_owned());
                }
            }
        }
    }
    ids
}

fn collect_paths(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        // `corpus/` is local, git-ignored reference data whose names we do not choose.
        if matches!(
            file_name.as_str(),
            "target" | ".git" | "corpus" | "naming-denylist.txt"
        ) {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            // Always '/' so names match the manifest on Windows too.
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
        if path.is_dir() {
            collect_paths(root, &path, out);
        }
    }
}

fn check_assets(root: &Path, problems: &mut Vec<String>) {
    let manifest = fs::read_to_string(root.join("assets/manifest.toml")).unwrap_or_default();
    let allowed: Vec<String> = manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix("allowed"))
        .map(|v| v.split('"').skip(1).step_by(2).map(str::to_owned).collect())
        .unwrap_or_default();
    if allowed.is_empty() {
        problems.push("assets/manifest.toml has no `allowed` licence list".to_owned());
    }
    // (path, licence) pairs from [[asset]] entries.
    let mut listed: BTreeMap<String, String> = BTreeMap::new();
    let mut path = None;
    for line in manifest
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
    {
        if let Some(v) = line.strip_prefix("path") {
            path = Some(
                v.trim_start_matches([' ', '='])
                    .trim_matches('"')
                    .to_owned(),
            );
        } else if let Some(v) = line.strip_prefix("licence") {
            if let Some(p) = path.take() {
                listed.insert(
                    p,
                    v.trim_start_matches([' ', '='])
                        .trim_matches('"')
                        .to_owned(),
                );
            }
        }
    }
    for (p, licence) in &listed {
        if !allowed.contains(licence) {
            problems.push(format!(
                "asset {p}: licence `{licence}` is not in the allowed list"
            ));
        }
        if !root.join(p).is_file() {
            problems.push(format!("asset {p}: listed in the manifest but missing"));
        }
    }
    let mut files = Vec::new();
    collect_paths(root, &root.join("assets"), &mut files);
    for f in files {
        let is_file = root.join(&f).is_file();
        if is_file && f != "assets/manifest.toml" && !listed.contains_key(&f) {
            problems.push(format!("asset {f}: not listed in assets/manifest.toml"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words() -> Vec<String> {
        ["ass", "anal", "obscene"].map(str::to_owned).to_vec()
    }

    #[test]
    fn whole_words_and_long_compounds_are_caught() {
        assert_eq!(offending_word("my.ass.file", &words()), Some("ass"));
        assert_eq!(offending_word("ob-obscene.toml", &words()), Some("obscene"));
        assert_eq!(offending_word("design.obscene", &words()), Some("obscene"));
    }

    #[test]
    fn ordinary_words_pass() {
        assert_eq!(
            offending_word("assembly-analysis.optobench", &words()),
            None
        );
        assert_eq!(offending_word("optobench-trace", &words()), None);
    }
}
