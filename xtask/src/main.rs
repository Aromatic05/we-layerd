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
        Some("install") => install(parse_install_args(args.collect())),
        Some(other) => panic!("unsupported xtask command: {other}"),
        None => panic!("usage: cargo xtask install [--prefix /usr]"),
    }
}

struct InstallArgs {
    prefix: Option<PathBuf>,
}

fn parse_install_args(args: Vec<String>) -> InstallArgs {
    let mut prefix = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--prefix" => {
                let value = iter.next().expect("--prefix requires a value");
                prefix = Some(expand_tilde(&value));
            }
            other => panic!("unsupported install argument: {other}"),
        }
    }

    if let Some(prefix) = &prefix {
        if !prefix.is_absolute() {
            panic!("install prefix must be absolute after expansion: {}", prefix.display());
        }
    }

    InstallArgs { prefix }
}

fn install(args: InstallArgs) {
    let workspace_root =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"))
            .parent()
            .expect("xtask manifest must live under workspace root")
            .to_path_buf();
    let effective_prefix = args
        .prefix
        .clone()
        .or_else(|| read_configured_prefix(&workspace_root).ok())
        .unwrap_or_else(|| expand_tilde("~/.local"));

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
        fs::remove_dir_all(destination).unwrap_or_else(|err| {
            panic!("failed to remove {}: {}", destination.display(), err)
        });
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
        let entry =
            entry.unwrap_or_else(|err| panic!("failed to read entry in {}: {}", source.display(), err));
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
