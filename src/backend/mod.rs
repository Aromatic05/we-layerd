pub(crate) mod gnome;
pub(crate) mod layer_shell;
pub(crate) mod traits;
pub(crate) mod wayland_common;

use crate::backend::traits::{BackendKind, WallpaperBackend};

pub(crate) fn create_backend(kind: BackendKind) -> Box<dyn WallpaperBackend> {
    match kind {
        BackendKind::LayerShell => Box::new(layer_shell::backend::LayerShellBackend),
        BackendKind::Gnome => Box::new(gnome::backend::GnomeBackend),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::{create_backend, gnome, traits::BackendKind};

    #[test]
    fn factory_creates_layer_shell_backend() {
        let backend = create_backend(BackendKind::LayerShell);
        assert_eq!(backend.kind(), BackendKind::LayerShell);
        let capabilities = backend.capabilities();
        assert!(capabilities.supports_dmabuf);
        assert!(capabilities.supports_shm);
        assert!(!capabilities.needs_external_extension);
        assert!(capabilities.owns_wayland_surface);
    }

    #[test]
    fn factory_creates_gnome_backend() {
        let backend = create_backend(BackendKind::Gnome);
        assert_eq!(backend.kind(), BackendKind::Gnome);
        let capabilities = backend.capabilities();
        assert!(capabilities.needs_external_extension);
        assert!(!capabilities.owns_wayland_surface);
    }

    #[test]
    fn gnome_backend_reports_clear_extension_error() {
        let err = gnome::dbus::ping_extension("io.github.weLayerd.DoesNotExist")
            .expect_err("missing extension must fail");
        assert_eq!(
            err.to_string(),
            "GNOME backend selected but we-layerd GNOME Shell extension is not reachable"
        );
    }

    #[test]
    fn gnome_protocol_xml_exists_and_matches_constants() {
        let xml_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("contrib/gnome-shell-extension/we-layerd@aromatic/protocol/io.github.weLayerd.Gnome.xml");
        let xml = fs::read_to_string(&xml_path).expect("protocol XML must exist");

        assert!(xml.contains("io.github.weLayerd.Gnome.WindowBridge"));
        assert!(xml.contains("io.github.weLayerd.Gnome.VideoBridge"));
        assert!(xml.contains(super::gnome::protocol::PING_METHOD));
        assert!(xml.contains(super::gnome::protocol::REGISTER_WINDOW_METHOD));
        assert!(xml.contains(super::gnome::protocol::UNREGISTER_WINDOW_METHOD));
        assert!(xml.contains(super::gnome::protocol::WINDOW_BRIDGE_INTERFACE));
        assert!(xml.contains(super::gnome::protocol::VIDEO_BRIDGE_INTERFACE));
    }

    #[test]
    fn extension_js_no_longer_inlines_dbus_xml() {
        let extension_js = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("contrib/gnome-shell-extension/we-layerd@aromatic/extension.js"),
        )
        .expect("extension.js must exist");
        assert!(!extension_js.contains("<interface name="));
        assert!(extension_js.contains("protocol/io.github.weLayerd.Gnome.xml"));
    }

    #[test]
    fn backend_architecture_boundaries_are_enforced_in_source_tree() {
        let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let layer_shell_type = ["Zwlr", "LayerShellV1"].concat();
        let layer_surface_type = ["Zwlr", "LayerSurfaceV1"].concat();
        let layer_shell_module = ["wayland_protocols_wlr", "layer_shell"].join("::");
        let legacy_entry = ["run_renderer", "background_surface"].join("_");
        let layer_needles =
            [layer_shell_type.as_str(), layer_surface_type.as_str(), layer_shell_module.as_str()];

        let layer_shell_refs = find_refs(&src_root, &layer_needles);

        assert!(!layer_shell_refs.is_empty());
        for path in layer_shell_refs {
            assert!(
                path.starts_with(src_root.join("backend/layer_shell")),
                "layer-shell protocol leaked into {}",
                path.display()
            );
        }

        assert!(find_refs(&src_root.join("runtime"), &layer_needles[..2]).is_empty());
        assert!(find_refs(&src_root.join("backend/wayland_common"), &layer_needles).is_empty());
        assert!(find_refs(&src_root.join("backend/gnome"), &layer_needles).is_empty());
        assert!(
            find_refs(&src_root, &[legacy_entry.as_str()]).is_empty(),
            "legacy background-surface entry must be removed"
        );
    }

    fn find_refs(root: &Path, needles: &[&str]) -> Vec<PathBuf> {
        let mut hits = Vec::new();
        collect_rs_files(root, &mut hits);
        hits.into_iter()
            .filter(|path| {
                if path.ends_with("src/backend/mod.rs") {
                    return false;
                }
                let content = fs::read_to_string(path).expect("source file must be readable");
                needles.iter().any(|needle| content.contains(needle))
            })
            .collect()
    }

    fn collect_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
        if root.is_file() {
            out.push(root.to_path_buf());
            return;
        }
        for entry in fs::read_dir(root).expect("directory must be readable") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}
