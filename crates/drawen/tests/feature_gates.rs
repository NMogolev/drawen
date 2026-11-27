//! Packaging gates for the facade feature set.
//!
//! The release checklist in `docs/RELEASING.md` explains what a release must
//! verify. These tests are the part of it that can be enforced: they fail when
//! `--no-default-features` starts pulling a gated dependency, when the default
//! feature set stops enabling a renderer, or when the workspace resolves two
//! `wgpu` versions.
//!
//! The dependency graph is read with `cargo tree` instead of `cargo metadata`
//! so the tests see the same resolution a consumer gets.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

/// Crates that must not appear in the headless facade.
const GATED_EXACT: &[&str] = &[
    "accesskit_winit",
    "cpal",
    "cubex",
    "drawen-browser",
    "image",
];

/// Prefixes of crates that must not appear in the headless facade.
const GATED_PREFIXES: &[&str] = &[
    "drawen-servo",
    "naga",
    "paleo",
    "servo",
    "symphonia",
    "wgpu",
    "windows",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the facade lives in crates/drawen")
        .to_path_buf()
}

fn cargo_tree(args: &[&str]) -> BTreeSet<String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["tree", "--prefix", "none"])
        .args(args)
        .current_dir(workspace_root())
        .output()
        .expect("cargo tree runs");
    assert!(
        output.status.success(),
        "cargo tree {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("cargo tree prints UTF-8")
        .lines()
        .filter_map(|line| line.split_once(" v").map(|(name, _)| name.to_owned()))
        .collect()
}

fn is_gated(name: &str) -> bool {
    GATED_EXACT.contains(&name) || GATED_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

#[test]
fn no_default_features_stay_headless() {
    let tree = cargo_tree(&["-e", "normal", "-p", "drawen", "--no-default-features"]);
    let leaked: Vec<&str> = tree
        .iter()
        .map(String::as_str)
        .filter(|name| is_gated(name))
        .collect();
    assert!(
        leaked.is_empty(),
        "drawen --no-default-features pulled gated dependencies: {leaked:?}"
    );
}

#[test]
fn default_features_enable_the_renderers_and_media() {
    let tree = cargo_tree(&["-e", "normal", "-p", "drawen"]);
    for crate_name in [
        "cubex",
        "drawen-browser",
        "drawen-wgpu",
        "image",
        "paleo",
        "wgpu",
    ] {
        assert!(
            tree.contains(crate_name),
            "the default feature set no longer enables {crate_name}"
        );
    }
    for crate_name in ["cpal", "drawen-director"] {
        assert!(
            !tree.contains(crate_name),
            "{crate_name} must stay opt-in, but the default feature set enables it"
        );
    }
}

#[test]
fn the_gpu_and_media_stacks_stay_independent() {
    let gpu = cargo_tree(&[
        "-e",
        "normal",
        "-p",
        "drawen",
        "--no-default-features",
        "--features",
        "gpu",
    ]);
    assert!(gpu.contains("wgpu") && gpu.contains("drawen-wgpu"));
    for crate_name in ["image", "paleo", "symphonia-core"] {
        assert!(
            !gpu.contains(crate_name),
            "the gpu feature pulls {crate_name}"
        );
    }

    let media = cargo_tree(&[
        "-e",
        "normal",
        "-p",
        "drawen",
        "--no-default-features",
        "--features",
        "media",
    ]);
    assert!(media.contains("paleo"));
    for crate_name in ["drawen-browser", "drawen-wgpu", "wgpu"] {
        assert!(
            !media.contains(crate_name),
            "the media feature pulls {crate_name}"
        );
    }
}

#[test]
fn the_cubex_feature_stays_independent() {
    let tree = cargo_tree(&[
        "-e",
        "normal",
        "-p",
        "drawen",
        "--no-default-features",
        "--features",
        "cubex",
    ]);
    assert!(tree.contains("cubex"));
    for crate_name in ["drawen-browser", "drawen-wgpu", "image", "paleo", "wgpu"] {
        assert!(
            !tree.contains(crate_name),
            "the cubex feature pulls {crate_name}"
        );
    }
}

#[test]
fn the_workspace_resolves_one_wgpu_version() {
    let lockfile = std::fs::read_to_string(workspace_root().join("Cargo.lock"))
        .expect("the workspace has a lock file");
    for crate_name in ["naga", "wgpu", "wgpu-core", "wgpu-hal"] {
        let versions = lockfile_versions(&lockfile, crate_name);
        assert_eq!(
            versions.len(),
            1,
            "{crate_name} must resolve to one version, found {versions:?}"
        );
    }
}

fn lockfile_versions(lockfile: &str, crate_name: &str) -> Vec<String> {
    let mut versions = Vec::new();
    let mut matches_name = false;
    for line in lockfile.lines() {
        if let Some(rest) = line.strip_prefix("name = \"") {
            matches_name = rest.trim_end_matches('"') == crate_name;
        } else if matches_name && let Some(rest) = line.strip_prefix("version = \"") {
            versions.push(rest.trim_end_matches('"').to_owned());
            matches_name = false;
        }
    }
    versions
}
