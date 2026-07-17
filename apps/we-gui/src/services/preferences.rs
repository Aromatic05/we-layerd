use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::domain::i18n::Language;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GuiPreferences {
    pub(crate) language: Language,
}

pub(crate) fn path() -> Option<PathBuf> {
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|value| value.is_absolute())
    {
        return Some(base.join("we-layerd/gui.toml"));
    }

    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(".config/we-layerd/gui.toml"))
}

pub(crate) fn load(path: &Path) -> GuiPreferences {
    let language = fs::read_to_string(path)
        .ok()
        .and_then(|contents| toml::from_str::<toml::Value>(&contents).ok())
        .and_then(|document| document.get("language")?.as_str().map(str::to_owned))
        .and_then(|tag| Language::from_tag(&tag))
        .unwrap_or_default();

    GuiPreferences { language }
}

pub(crate) fn save(path: &Path, preferences: GuiPreferences) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "preferences path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    let file_name = path.file_name().and_then(|value| value.to_str()).unwrap_or("gui.toml");
    let contents = format!("language = \"{}\"\n", preferences.language.tag());
    let (temporary, mut file) = create_temporary_file(parent, file_name)
        .map_err(|error| error.to_string())?;

    let write_result = (|| -> std::io::Result<()> {
        file.write_all(contents.as_bytes())?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }

    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }

    #[cfg(unix)]
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }

    Ok(())
}

fn create_temporary_file(parent: &Path, file_name: &str) -> std::io::Result<(PathBuf, fs::File)> {
    for _ in 0..128 {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temporary) {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique preferences temporary file",
    ))
}

#[cfg(test)]
mod tests {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    use super::{load, save, GuiPreferences};
    use crate::domain::i18n::Language;

    fn temporary_preferences_path(name: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("we-gui-preferences-{name}-{suffix}/gui.toml"))
    }

    #[test]
    fn missing_and_corrupt_preferences_default_to_english() {
        let path = temporary_preferences_path("defaults");
        assert_eq!(load(&path).language, Language::English);

        fs::create_dir_all(path.parent().expect("preferences parent"))
            .expect("create preferences directory");
        fs::write(&path, "language = [not valid").expect("write corrupt preferences");
        assert_eq!(load(&path).language, Language::English);

        fs::write(&path, "language = \"zh_CN\"\n").expect("write unsupported preferences");
        assert_eq!(load(&path).language, Language::English);

        fs::remove_dir_all(path.parent().expect("preferences parent"))
            .expect("remove preferences directory");
    }

    #[test]
    fn simplified_chinese_round_trips_with_bcp47_tag() {
        let path = temporary_preferences_path("roundtrip");
        save(&path, GuiPreferences { language: Language::English })
            .expect("save initial preferences");
        save(
            &path,
            GuiPreferences { language: Language::SimplifiedChinese },
        )
        .expect("save preferences");

        assert_eq!(load(&path).language, Language::SimplifiedChinese);
        assert_eq!(fs::read_to_string(&path).expect("read preferences"), "language = \"zh-Hans\"\n");
        assert!(fs::read_dir(path.parent().expect("preferences parent"))
            .expect("read preferences directory")
            .all(|entry| !entry.expect("directory entry").file_name().to_string_lossy().ends_with(".tmp")));

        fs::remove_dir_all(path.parent().expect("preferences parent"))
            .expect("remove preferences directory");
    }
}
