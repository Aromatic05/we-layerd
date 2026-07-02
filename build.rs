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
    for install_root in &install_roots {
        fs::create_dir_all(install_root.join("lib"))
            .expect("failed to create renderer install dir");
    }
    ensure_recursive_submodules(&upstream_root);
    reset_cmake_cache_if_source_changed(&build_root, &upstream_root)
        .expect("failed to reset stale cmake cache");

    run(
        Command::new("cmake")
            .arg("-S")
            .arg(&upstream_root)
            .arg("-B")
            .arg(&build_root)
            .arg("-DCMAKE_BUILD_TYPE=RelWithDebInfo"),
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

    if !built_library.exists() {
        panic!(
            "expected built renderer library at {}, but it was not produced",
            built_library.display()
        );
    }

    for install_root in &install_roots {
        let installed_library = install_root.join("lib/libwallpaper-engine-renderer.so");
        copy_if_different(&built_library, &installed_library)
            .expect("failed to copy renderer library into install root");
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

fn copy_if_different(from: &Path, to: &Path) -> std::io::Result<()> {
    let replace = match (fs::metadata(from), fs::metadata(to)) {
        (Ok(src), Ok(dst)) => {
            src.len() != dst.len()
                || src.modified().ok().zip(dst.modified().ok()).map(|(a, b)| a > b).unwrap_or(true)
        }
        (Ok(_), Err(_)) => true,
        (Err(err), _) => return Err(err),
    };

    if replace {
        fs::copy(from, to)?;
    }
    Ok(())
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
