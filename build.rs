use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.gitmodules");
    println!("cargo:rerun-if-changed=third_party/wallpaper-engine-renderer/CMakeLists.txt");
    println!("cargo:rerun-if-changed=third_party/wallpaper-engine-renderer/src");
    println!("cargo:rerun-if-changed=third_party/wallpaper-engine-renderer/include");
    println!(
        "cargo:rerun-if-changed=third_party/wallpaper-engine-renderer/standalone_layer_view"
    );

    let workspace_root =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let upstream_root = workspace_root.join("third_party/wallpaper-engine-renderer");
    if !upstream_root.exists() {
        panic!(
            "missing upstream renderer repository at {}",
            upstream_root.display()
        );
    }

    let install_roots = install_roots(&workspace_root);
    let build_root = workspace_root.join("target/we-renderer-upstream/build");
    let built_library = build_root.join("src/libwallpaper-engine-renderer.so");
    ensure_recursive_submodules(&upstream_root);
    reset_cmake_cache_if_source_changed(&build_root, &upstream_root)
        .expect("failed to reset stale cmake cache");

    run(
        Command::new("cmake")
            .arg("-S")
            .arg(&upstream_root)
            .arg("-B")
            .arg(&build_root)
            .arg("-DCMAKE_BUILD_TYPE=Release"),
        "configure upstream wallpaper-engine-renderer",
    );

    run(
        Command::new("cmake")
            .arg("--build")
            .arg(&build_root)
            .arg("--target")
            .arg("wallpaper-engine-renderer")
            .arg("--parallel"),
        "build upstream wallpaper-engine-renderer",
    );

    run(
        Command::new("strip")
            .arg("--strip-unneeded")
            .arg(&built_library),
        "strip upstream wallpaper-engine-renderer",
    );

    if !built_library.exists() {
        panic!(
            "expected built renderer library at {}, but it was not produced",
            built_library.display()
        );
    }

    for install_root in &install_roots {
        run(
            Command::new("cmake")
                .arg("--install")
                .arg(&build_root)
                .arg("--prefix")
                .arg(install_root),
            &format!(
                "install upstream wallpaper-engine-renderer into {}",
                install_root.display()
            ),
        );
    }

    println!(
        "cargo:rustc-env=WE_LAYERD_RENDERER_INSTALL_ROOT={}",
        install_roots[0].display()
    );
}

fn install_roots(workspace_root: &Path) -> Vec<PathBuf> {
    let home = env::var_os("HOME").expect("HOME must be set");
    vec![
        workspace_root.join("target/we-renderer-upstream/install"),
        PathBuf::from(home).join(".local/bin"),
    ]
}

fn run(command: &mut Command, description: &str) {
    let status = command.status().unwrap_or_else(|err| {
        panic!("failed to {}: {}", description, err);
    });
    if !status.success() {
        panic!("failed to {}: {}", description, status);
    }
}

fn reset_cmake_cache_if_source_changed(build_root: &Path, upstream_root: &Path) -> std::io::Result<()> {
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
