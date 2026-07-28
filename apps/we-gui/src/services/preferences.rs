use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::domain::{i18n::Language, settings::is_shuffle_interval_ms};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GuiPreferences {
    pub(crate) language: Language,
    pub(crate) shuffle_enabled: bool,
    pub(crate) shuffle_interval_ms: u32,
    pub(crate) shuffle_include_video: bool,
    pub(crate) shuffle_include_scene: bool,
    pub(crate) shuffle_include_web: bool,
}

impl Default for GuiPreferences {
    fn default() -> Self {
        Self {
            language: Language::default(),
            shuffle_enabled: false,
            shuffle_interval_ms: 1_800_000,
            shuffle_include_video: true,
            shuffle_include_scene: true,
            shuffle_include_web: true,
        }
    }
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
    let Some(document) = fs::read_to_string(path)
        .ok()
        .and_then(|contents| toml::from_str::<toml::Value>(&contents).ok())
    else {
        return GuiPreferences::default();
    };
    let defaults = GuiPreferences::default();
    let language = document
        .get("language")
        .and_then(toml::Value::as_str)
        .and_then(Language::from_tag)
        .unwrap_or(defaults.language);
    let shuffle_interval_ms = document
        .get("shuffle_interval_ms")
        .and_then(toml::Value::as_integer)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| is_shuffle_interval_ms(*value))
        .or_else(|| {
            document
                .get("shuffle_interval_minutes")
                .and_then(toml::Value::as_integer)
                .and_then(|value| u32::try_from(value).ok())
                .and_then(|minutes| minutes.checked_mul(60_000))
                .filter(|value| is_shuffle_interval_ms(*value))
        })
        .unwrap_or(defaults.shuffle_interval_ms);

    GuiPreferences {
        language,
        shuffle_enabled: preference_bool(&document, "shuffle_enabled", defaults.shuffle_enabled),
        shuffle_interval_ms,
        shuffle_include_video: preference_bool(
            &document,
            "shuffle_include_video",
            defaults.shuffle_include_video,
        ),
        shuffle_include_scene: preference_bool(
            &document,
            "shuffle_include_scene",
            defaults.shuffle_include_scene,
        ),
        shuffle_include_web: preference_bool(
            &document,
            "shuffle_include_web",
            defaults.shuffle_include_web,
        ),
    }
}

pub(crate) fn save(path: &Path, preferences: GuiPreferences) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "preferences path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    let file_name = path.file_name().and_then(|value| value.to_str()).unwrap_or("gui.toml");
    let contents = format!(
        concat!(
            "language = \"{}\"\n",
            "shuffle_enabled = {}\n",
            "shuffle_interval_ms = {}\n",
            "shuffle_include_video = {}\n",
            "shuffle_include_scene = {}\n",
            "shuffle_include_web = {}\n",
        ),
        preferences.language.tag(),
        preferences.shuffle_enabled,
        preferences.shuffle_interval_ms,
        preferences.shuffle_include_video,
        preferences.shuffle_include_scene,
        preferences.shuffle_include_web,
    );
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

fn preference_bool(document: &toml::Value, key: &str, default: bool) -> bool {
    document.get(key).and_then(toml::Value::as_bool).unwrap_or(default)
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
        save(&path, GuiPreferences { language: Language::English, ..GuiPreferences::default() })
            .expect("save initial preferences");
        save(
            &path,
            GuiPreferences {
                language: Language::SimplifiedChinese,
                shuffle_enabled: true,
                shuffle_interval_ms: 123_456,
                shuffle_include_video: true,
                shuffle_include_scene: false,
                shuffle_include_web: true,
            },
        )
        .expect("save preferences");

        let loaded = load(&path);
        assert_eq!(loaded.language, Language::SimplifiedChinese);
        assert!(loaded.shuffle_enabled);
        assert_eq!(loaded.shuffle_interval_ms, 123_456);
        assert!(loaded.shuffle_include_video);
        assert!(!loaded.shuffle_include_scene);
        assert!(loaded.shuffle_include_web);
        assert_eq!(
            fs::read_to_string(&path).expect("read preferences"),
            concat!(
                "language = \"zh-Hans\"\n",
                "shuffle_enabled = true\n",
                "shuffle_interval_ms = 123456\n",
                "shuffle_include_video = true\n",
                "shuffle_include_scene = false\n",
                "shuffle_include_web = true\n",
            )
        );
        assert!(fs::read_dir(path.parent().expect("preferences parent"))
            .expect("read preferences directory")
            .all(|entry| !entry.expect("directory entry").file_name().to_string_lossy().ends_with(".tmp")));

        fs::remove_dir_all(path.parent().expect("preferences parent"))
            .expect("remove preferences directory");
    }

    #[test]
    fn legacy_preferences_get_shuffle_defaults() {
        let path = temporary_preferences_path("legacy");
        fs::create_dir_all(path.parent().expect("preferences parent"))
            .expect("create preferences directory");
        fs::write(&path, "language = \"zh-Hans\"\n").expect("write legacy preferences");

        let loaded = load(&path);
        assert_eq!(loaded.language, Language::SimplifiedChinese);
        assert!(!loaded.shuffle_enabled);
        assert_eq!(loaded.shuffle_interval_ms, 1_800_000);
        assert!(loaded.shuffle_include_video);
        assert!(loaded.shuffle_include_scene);
        assert!(loaded.shuffle_include_web);

        fs::remove_dir_all(path.parent().expect("preferences parent"))
            .expect("remove preferences directory");
    }

    #[test]
    fn minute_based_shuffle_interval_migrates_to_milliseconds() {
        let path = temporary_preferences_path("minute-migration");
        fs::create_dir_all(path.parent().expect("preferences parent"))
            .expect("create preferences directory");
        fs::write(&path, "shuffle_interval_minutes = 15\n")
            .expect("write minute preferences");

        assert_eq!(load(&path).shuffle_interval_ms, 900_000);

        fs::remove_dir_all(path.parent().expect("preferences parent"))
            .expect("remove preferences directory");
    }
}
