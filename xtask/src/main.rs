use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

const GNOME_EXTENSION_UUID: &str = "we-layerd@aromatic";

fn main() {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("install") => install(parse_prefix_args("install", args.collect())),
        Some("uninstall") => uninstall(parse_prefix_args("uninstall", args.collect())),
        Some(other) => panic!("unsupported xtask command: {other}"),
        None => panic!("usage: cargo xtask <install|uninstall> [--prefix /usr]"),
    }
}

struct PrefixArgs {
    prefix: Option<PathBuf>,
}

fn parse_prefix_args(command: &str, args: Vec<String>) -> PrefixArgs {
    let mut prefix = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--prefix" => {
                let value = iter.next().expect("--prefix requires a value");
                prefix = Some(expand_tilde(&value));
            }
            other => panic!("unsupported {command} argument: {other}"),
        }
    }

    if let Some(prefix) = &prefix {
        if !prefix.is_absolute() {
            panic!("{command} prefix must be absolute after expansion: {}", prefix.display());
        }
    }

    PrefixArgs { prefix }
}

fn install(args: PrefixArgs) {
    let workspace_root = workspace_root();
    let effective_prefix = effective_prefix(&workspace_root, args.prefix.as_deref());

    run(
        configure_release_build(
            &mut Command::new("cargo"),
            &workspace_root,
            args.prefix.as_deref(),
        ),
        "build release binaries for installation",
    );

    let stage_root = workspace_root.join("target/we-renderer-upstream/install");
    let install_root = resolve_install_root(&effective_prefix);

    install_artifact(
        &workspace_root.join("target/release/we-layerd"),
        &install_root.join("bin/we-layerd"),
    );
    install_artifact(
        &workspace_root.join("target/release/we-gui"),
        &install_root.join("bin/we-gui"),
    );
    install_artifact(
        &stage_root.join("lib/libwallpaper-engine-renderer.so"),
        &install_root.join("lib/libwallpaper-engine-renderer.so"),
    );
    install_artifact(
        &stage_root.join("lib/we-cef-helper"),
        &install_root.join("lib/we-cef-helper"),
    );
    install_tree(
        &workspace_root.join("contrib/gnome-shell-extension").join(GNOME_EXTENSION_UUID),
        &install_root.join("share/gnome-shell/extensions").join(GNOME_EXTENSION_UUID),
    );
}

fn uninstall(args: PrefixArgs) {
    let workspace_root = workspace_root();
    let effective_prefix = effective_prefix(&workspace_root, args.prefix.as_deref());
    let install_root = resolve_install_root(&effective_prefix);
    remove_installation(&install_root);
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"))
        .parent()
        .expect("xtask manifest must live under workspace root")
        .to_path_buf()
}

fn effective_prefix(workspace_root: &Path, explicit_prefix: Option<&Path>) -> PathBuf {
    explicit_prefix
        .map(Path::to_path_buf)
        .or_else(|| read_configured_prefix(workspace_root).ok())
        .unwrap_or_else(|| expand_tilde("~/.local"))
}

fn uninstall_targets(install_root: &Path) -> Vec<PathBuf> {
    vec![
        install_root.join("bin/we-layerd"),
        install_root.join("bin/we-gui"),
        install_root.join("lib/libwallpaper-engine-renderer.so"),
        install_root.join("lib/we-cef-helper"),
        install_root.join("share/gnome-shell/extensions").join(GNOME_EXTENSION_UUID),
    ]
}

fn remove_installation(install_root: &Path) {
    let targets = uninstall_targets(install_root);
    for target in &targets[..4] {
        remove_file_if_present(target);
    }
    remove_tree_if_present(&targets[4]);
}

fn remove_file_if_present(path: &Path) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => panic!("failed to inspect {}: {}", path.display(), err),
    };
    if metadata.is_dir() {
        panic!("refusing to remove directory at installed file path {}", path.display());
    }
    fs::remove_file(path)
        .unwrap_or_else(|err| panic!("failed to remove {}: {}", path.display(), err));
    println!("Removed {}", path.display());
}

fn remove_tree_if_present(path: &Path) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => panic!("failed to inspect {}: {}", path.display(), err),
    };
    if metadata.file_type().is_symlink() {
        fs::remove_file(path)
            .unwrap_or_else(|err| panic!("failed to remove symlink {}: {}", path.display(), err));
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)
            .unwrap_or_else(|err| panic!("failed to remove {}: {}", path.display(), err));
    } else {
        panic!("refusing to remove non-directory at installed tree path {}", path.display());
    }
    println!("Removed {}", path.display());
}

fn resolve_install_root(prefix: &Path) -> PathBuf {
    match env::var_os("DESTDIR") {
        Some(destdir) if !destdir.is_empty() => {
            let relative_prefix =
                prefix.strip_prefix("/").expect("absolute prefix must start with '/'");
            PathBuf::from(destdir).join(relative_prefix)
        }
        _ => prefix.to_path_buf(),
    }
}

fn configure_release_build<'a>(
    command: &'a mut Command,
    workspace_root: &Path,
    prefix: Option<&Path>,
) -> &'a mut Command {
    let command = command
        .arg("build")
        .arg("--release")
        .arg("-p")
        .arg("we-layerd")
        .arg("-p")
        .arg("we-gui")
        .current_dir(workspace_root);
    let effective_prefix = prefix
        .map(Path::to_path_buf)
        .or_else(|| read_configured_prefix(workspace_root).ok())
        .unwrap_or_else(|| expand_tilde("~/.local"));
    command.env("WE_LAYERD_INSTALL_PREFIX", &effective_prefix);
    command
}

fn read_configured_prefix(workspace_root: &Path) -> Result<PathBuf, String> {
    let path = workspace_root.join("target/we-renderer-upstream/install-prefix.txt");
    let raw = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read {}: {}", path.display(), err))?;
    Ok(PathBuf::from(raw.trim()))
}

fn install_artifact(source: &Path, destination: &Path) {
    if !source.is_file() {
        panic!("missing install artifact {}", source.display());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|err| panic!("failed to create {}: {}", parent.display(), err));
    }

    fs::copy(source, destination).unwrap_or_else(|err| {
        panic!("failed to copy {} to {}: {}", source.display(), destination.display(), err)
    });

    let mut permissions = fs::metadata(destination)
        .unwrap_or_else(|err| panic!("failed to stat {}: {}", destination.display(), err))
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(destination, permissions)
        .unwrap_or_else(|err| panic!("failed to chmod {}: {}", destination.display(), err));

    run(
        Command::new("strip").arg("--strip-unneeded").arg(destination),
        &format!("strip {}", destination.display()),
    );
}

fn install_tree(source: &Path, destination: &Path) {
    if !source.is_dir() {
        panic!("missing install tree {}", source.display());
    }

    if destination.exists() {
        fs::remove_dir_all(destination)
            .unwrap_or_else(|err| panic!("failed to remove {}: {}", destination.display(), err));
    }

    copy_tree(source, destination);
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination)
        .unwrap_or_else(|err| panic!("failed to create {}: {}", destination.display(), err));
    set_mode(destination, 0o755);

    let entries = fs::read_dir(source)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", source.display(), err));
    for entry in entries {
        let entry = entry
            .unwrap_or_else(|err| panic!("failed to read entry in {}: {}", source.display(), err));
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().unwrap_or_else(|err| {
            panic!("failed to determine file type for {}: {}", source_path.display(), err)
        });

        if file_type.is_dir() {
            copy_tree(&source_path, &destination_path);
            continue;
        }
        if !file_type.is_file() {
            panic!("unsupported non-file entry in install tree: {}", source_path.display());
        }

        fs::copy(&source_path, &destination_path).unwrap_or_else(|err| {
            panic!(
                "failed to copy {} to {}: {}",
                source_path.display(),
                destination_path.display(),
                err
            )
        });
        set_mode(&destination_path, 0o644);
    }
}

fn set_mode(path: &Path, mode: u32) {
    let mut permissions = fs::metadata(path)
        .unwrap_or_else(|err| panic!("failed to stat {}: {}", path.display(), err))
        .permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions)
        .unwrap_or_else(|err| panic!("failed to chmod {}: {}", path.display(), err));
}

fn expand_tilde(raw: &str) -> PathBuf {
    if raw == "~" {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(raw)
}

fn run(command: &mut Command, description: &str) {
    let status =
        command.status().unwrap_or_else(|err| panic!("failed to {}: {}", description, err));
    if !status.success() {
        panic!("failed to {}: {}", description, status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_nanos();
        let path =
            env::temp_dir().join(format!("we-layerd-xtask-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).expect("failed to create test directory");
        path
    }

    #[test]
    fn uninstall_accepts_the_same_prefix_option_as_install() {
        let args = parse_prefix_args("uninstall", vec!["--prefix".into(), "/opt/we-layerd".into()]);
        assert_eq!(args.prefix, Some(PathBuf::from("/opt/we-layerd")));
    }

    #[test]
    fn uninstall_targets_match_every_artifact_installed_by_xtask() {
        let root = Path::new("/opt/we-layerd");
        assert_eq!(
            uninstall_targets(root),
            vec![
                root.join("bin/we-layerd"),
                root.join("bin/we-gui"),
                root.join("lib/libwallpaper-engine-renderer.so"),
                root.join("lib/we-cef-helper"),
                root.join("share/gnome-shell/extensions").join(GNOME_EXTENSION_UUID),
            ]
        );
    }

    #[test]
    fn uninstall_is_idempotent_and_preserves_unrelated_files() {
        let root = temporary_directory("uninstall");
        for target in uninstall_targets(&root).into_iter().take(4) {
            fs::create_dir_all(target.parent().expect("target must have a parent"))
                .expect("failed to create target parent");
            fs::write(&target, b"installed").expect("failed to create installed file");
        }
        let extension = root.join("share/gnome-shell/extensions").join(GNOME_EXTENSION_UUID);
        fs::create_dir_all(&extension).expect("failed to create extension directory");
        fs::write(extension.join("extension.js"), b"installed")
            .expect("failed to create extension file");
        let unrelated = root.join("bin/keep-me");
        fs::write(&unrelated, b"unrelated").expect("failed to create unrelated file");

        remove_installation(&root);
        remove_installation(&root);

        assert!(uninstall_targets(&root).iter().all(|path| !path.exists()));
        assert_eq!(fs::read(&unrelated).expect("unrelated file should remain"), b"unrelated");
        fs::remove_dir_all(root).expect("failed to clean test directory");
    }
}
