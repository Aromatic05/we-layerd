use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.gitmodules");
    println!("cargo:rerun-if-env-changed=CEF_ROOT");

    let workspace_root =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let upstream_root = workspace_root
        .parent()
        .expect("workspace root must have parent directory")
        .join("we-new/wallpaper-engine-renderer");
    if !upstream_root.exists() {
        panic!("missing upstream renderer repository at {}", upstream_root.display());
    }
    emit_upstream_rerun_hints(&upstream_root);

    let build_root = workspace_root.join("target/we-renderer-upstream/build");
    let install_root = workspace_root.join("target/we-renderer-upstream/install");
    ensure_recursive_submodules(&upstream_root);
    reset_cmake_cache_if_source_changed(&build_root, &upstream_root)
        .expect("failed to reset stale cmake cache");

    run(
        Command::new("cmake")
            .arg("-S")
            .arg(&upstream_root)
            .arg("-B")
            .arg(&build_root)
            .arg("-DCMAKE_BUILD_TYPE=Release")
            .arg(format!("-DCMAKE_INSTALL_PREFIX={}", install_root.display()))
            .arg("-DCMAKE_INSTALL_LIBDIR=lib")
            .arg("-DBUILD_WEWEB=ON"),
        "configure upstream wallpaper-engine-renderer",
    );

    ensure_cmake_cache_value(&build_root.join("CMakeCache.txt"), "BUILD_WEWEB:BOOL", "ON")
        .expect("failed to verify BUILD_WEWEB in CMakeCache.txt");

    for target in ["wallpaper-engine-renderer", "we-cef-helper"] {
        run(
            Command::new("cmake")
                .arg("--build")
                .arg(&build_root)
                .arg("--target")
                .arg(target)
                .arg("--parallel"),
            &format!("build upstream target {target}"),
        );
    }

    run(
        Command::new("cmake").arg("--install").arg(&build_root),
        &format!("install upstream wallpaper-engine-renderer into {}", install_root.display()),
    );

    let install_lib_dir = install_root.join("lib");
    let renderer_library = install_lib_dir.join("libwallpaper-engine-renderer.so");
    let cef_helper = install_lib_dir.join("we-cef-helper");

    for artifact in [&renderer_library, &cef_helper] {
        if !artifact.is_file() {
            panic!(
                "expected installed artifact at {}, but it was not produced",
                artifact.display()
            );
        }
    }

    for artifact in [&renderer_library, &cef_helper] {
        run(
            Command::new("strip").arg("--strip-unneeded").arg(artifact),
            &format!("strip {}", artifact.display()),
        );
    }

    println!("cargo:rustc-env=WE_LAYERD_RENDERER_INSTALL_ROOT={}", install_root.display());
}

fn emit_upstream_rerun_hints(upstream_root: &Path) {
    for path in [
        upstream_root.join("CMakeLists.txt"),
        upstream_root.join("src"),
        upstream_root.join("include"),
        upstream_root.join("standalone_layer_view"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn run(command: &mut Command, description: &str) {
    let status = command.status().unwrap_or_else(|err| {
        panic!("failed to {}: {}", description, err);
    });
    if !status.success() {
        panic!("failed to {}: {}", description, status);
    }
}

fn reset_cmake_cache_if_source_changed(
    build_root: &Path,
    upstream_root: &Path,
) -> std::io::Result<()> {
    let cache_path = build_root.join("CMakeCache.txt");
    if !cache_path.exists() {
        return Ok(());
    }

    let cache = fs::read_to_string(&cache_path)?;
    let expected = upstream_root.display().to_string();
    let has_expected_source = cache
        .lines()
        .find(|line| line.starts_with("CMAKE_HOME_DIRECTORY:INTERNAL="))
        .map(|line| line.ends_with(&expected))
        .unwrap_or(false);
    if !has_expected_source {
        fs::remove_dir_all(build_root)?;
    }
    Ok(())
}

fn ensure_recursive_submodules(upstream_root: &Path) {
    run(
        Command::new("git")
            .arg("-C")
            .arg(upstream_root)
            .arg("submodule")
            .arg("update")
            .arg("--init")
            .arg("--recursive"),
        "initialize wallpaper-engine-renderer recursive submodules",
    );
}

fn ensure_cmake_cache_value(cache_path: &Path, key: &str, expected: &str) -> std::io::Result<()> {
    let cache = fs::read_to_string(cache_path)?;
    let prefix = format!("{key}=");
    let value = cache.lines().find_map(|line| line.strip_prefix(&prefix)).unwrap_or("");
    if value != expected {
        panic!("expected {key}={expected} in {}, got {:?}", cache_path.display(), value);
    }
    Ok(())
}
