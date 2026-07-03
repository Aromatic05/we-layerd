use std::{collections::HashSet, path::PathBuf};

use anyhow::{anyhow, Result};

pub const RENDERER_LIBRARY_NAME: &str = "libwallpaper-engine-renderer.so";

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
    use std::path::PathBuf;

    use super::{expand_tilde, renderer_library_candidates};

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
}
