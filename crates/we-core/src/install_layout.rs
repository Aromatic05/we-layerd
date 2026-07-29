use std::{collections::HashSet, path::PathBuf};

use anyhow::{anyhow, Result};

pub const RENDERER_LIBRARY_NAME: &str = "libwallpaper-engine-renderer.so";
pub const RENDERER_LIBRARY_OVERRIDE_ENV: &str = "WE_LAYERD_RENDERER_LIBRARY_PATH";

pub fn expand_tilde(raw: &str) -> PathBuf {
    if raw == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }

    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }

    PathBuf::from(raw)
}

pub fn renderer_library_candidates(configured_path: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    push_unique(&mut candidates, &mut seen, normalized_candidate(configured_path));

    if let Some(home) = std::env::var_os("HOME") {
        push_unique(
            &mut candidates,
            &mut seen,
            Some(PathBuf::from(home).join(".local/lib").join(RENDERER_LIBRARY_NAME)),
        );
    }

    push_unique(
        &mut candidates,
        &mut seen,
        Some(PathBuf::from("/usr/lib").join(RENDERER_LIBRARY_NAME)),
    );

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(bin_dir) = current_exe.parent() {
            push_unique(
                &mut candidates,
                &mut seen,
                Some(bin_dir.join("../lib").join(RENDERER_LIBRARY_NAME)),
            );
            push_unique(&mut candidates, &mut seen, Some(bin_dir.join(RENDERER_LIBRARY_NAME)));
        }
    }

    if cfg!(debug_assertions) {
        if let Some(install_root) = std::env::var_os("WE_LAYERD_RENDERER_INSTALL_ROOT") {
            push_unique(
                &mut candidates,
                &mut seen,
                Some(PathBuf::from(install_root).join("lib").join(RENDERER_LIBRARY_NAME)),
            );
        }

        if let Ok(current_dir) = std::env::current_dir() {
            push_unique(&mut candidates, &mut seen, Some(current_dir.join(RENDERER_LIBRARY_NAME)));
            push_unique(
                &mut candidates,
                &mut seen,
                Some(
                    current_dir
                        .join("target/we-renderer-upstream/install/lib")
                        .join(RENDERER_LIBRARY_NAME),
                ),
            );
            push_unique(
                &mut candidates,
                &mut seen,
                Some(current_dir.join("build").join(RENDERER_LIBRARY_NAME)),
            );
        }
    }

    candidates
}

pub fn resolve_renderer_library(configured_path: &str) -> Result<PathBuf> {
    let forced_override = std::env::var_os(RENDERER_LIBRARY_OVERRIDE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    resolve_renderer_library_with_override(configured_path, forced_override)
}

fn resolve_renderer_library_with_override(
    configured_path: &str,
    forced_override: Option<PathBuf>,
) -> Result<PathBuf> {
    if let Some(forced_path) = forced_override {
        if forced_path.is_file() {
            return Ok(forced_path);
        }
        return Err(anyhow!(
            "renderer library forced by {env} does not exist or is not a file: {path}",
            env = RENDERER_LIBRARY_OVERRIDE_ENV,
            path = forced_path.display()
        ));
    }

    let candidates = renderer_library_candidates(configured_path);
    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    Err(anyhow!(
        "failed to resolve {name}; tried: {paths}",
        name = RENDERER_LIBRARY_NAME,
        paths = format_candidate_list(&candidates)
    ))
}

fn normalized_candidate(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(expand_tilde(trimmed))
    }
}

fn push_unique(
    candidates: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    candidate: Option<PathBuf>,
) {
    let Some(candidate) = candidate else {
        return;
    };
    if seen.insert(candidate.clone()) {
        candidates.push(candidate);
    }
}

fn format_candidate_list(candidates: &[PathBuf]) -> String {
    candidates
        .iter()
        .map(|candidate| candidate.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        expand_tilde, renderer_library_candidates, resolve_renderer_library_with_override,
    };

    fn temporary_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("we-layerd-install-layout-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).expect("failed to create temporary directory");
        path
    }

    #[test]
    fn expand_tilde_keeps_plain_paths() {
        assert_eq!(expand_tilde("/tmp/demo"), PathBuf::from("/tmp/demo"));
    }

    #[test]
    fn empty_configured_path_does_not_become_first_candidate() {
        let candidates = renderer_library_candidates("");
        assert!(!candidates.is_empty());
        assert_ne!(candidates[0], PathBuf::from(""));
    }

    #[test]
    fn forced_renderer_library_overrides_an_existing_configured_library() {
        let root = temporary_directory("forced-override");
        let forced = root.join("bundled-renderer.so");
        let configured = root.join("host-renderer.so");
        fs::write(&forced, b"bundled").expect("failed to create bundled renderer");
        fs::write(&configured, b"host").expect("failed to create configured renderer");

        let resolved = resolve_renderer_library_with_override(
            configured.to_str().expect("configured path must be UTF-8"),
            Some(forced.clone()),
        )
        .expect("forced renderer should resolve");

        assert_eq!(resolved, forced);
        fs::remove_dir_all(root).expect("failed to clean temporary directory");
    }

    #[test]
    fn missing_forced_renderer_library_does_not_fall_back_to_configured_library() {
        let root = temporary_directory("missing-forced-override");
        let forced = root.join("missing-bundled-renderer.so");
        let configured = root.join("host-renderer.so");
        fs::write(&configured, b"host").expect("failed to create configured renderer");

        let error = resolve_renderer_library_with_override(
            configured.to_str().expect("configured path must be UTF-8"),
            Some(forced.clone()),
        )
        .expect_err("missing forced renderer must fail closed");

        assert!(error.to_string().contains(&forced.display().to_string()));
        fs::remove_dir_all(root).expect("failed to clean temporary directory");
    }
}
