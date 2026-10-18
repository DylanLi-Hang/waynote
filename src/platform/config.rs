use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

use super::paths::Paths;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_color: String,
    pub image_max_pixels: u64,
    pub image_max_bytes: u64,
    pub image_downsample: bool,
    pub watch_debounce_ms: u64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_color: "yellow".to_string(),
            image_max_pixels: 4_000_000,
            image_max_bytes: 5_242_880,
            image_downsample: true,
            watch_debounce_ms: 400,
            extra: BTreeMap::new(),
        }
    }
}

pub fn load(paths: &Paths) -> Config {
    let config_path = paths.config_file();

    // If file doesn't exist, return defaults silently.
    if !config_path.exists() {
        return Config::default();
    }

    // Try to read and parse the file. If it fails, return defaults + warn.
    match std::fs::read_to_string(&config_path) {
        Ok(content) => match toml::from_str::<Config>(&content) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Error parsing config.toml: {}", e);
                Config::default()
            }
        },
        Err(e) => {
            eprintln!("Error reading config.toml: {}", e);
            Config::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_file_returns_all_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_str().unwrap().to_string();
        let paths = Paths::from_env(|k| match k {
            "XDG_CONFIG_HOME" => Some(base.clone()),
            _ => None,
        });

        let config = load(&paths);

        assert_eq!(config.default_color, "yellow");
        assert_eq!(config.image_max_pixels, 4_000_000);
        assert_eq!(config.image_max_bytes, 5_242_880);
        assert!(config.image_downsample);
        assert_eq!(config.watch_debounce_ms, 400);
        assert!(config.extra.is_empty());
    }

    #[test]
    fn partial_config_overrides_only_specified_keys() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_str().unwrap().to_string();
        let waynote_dir = std::path::Path::new(&base).join("waynote");
        std::fs::create_dir_all(&waynote_dir).unwrap();

        let config_file = waynote_dir.join("config.toml");
        std::fs::write(&config_file, r#"default_color = "blue""#).unwrap();

        let paths = Paths::from_env(|k| match k {
            "XDG_CONFIG_HOME" => Some(base.clone()),
            _ => None,
        });

        let config = load(&paths);

        assert_eq!(config.default_color, "blue"); // overridden
    }

    #[test]
    fn unknown_key_is_preserved_in_extra() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_str().unwrap().to_string();
        let waynote_dir = std::path::Path::new(&base).join("waynote");
        std::fs::create_dir_all(&waynote_dir).unwrap();

        let config_file = waynote_dir.join("config.toml");
        std::fs::write(&config_file, r#"wat = 1"#).unwrap();

        let paths = Paths::from_env(|k| match k {
            "XDG_CONFIG_HOME" => Some(base.clone()),
            _ => None,
        });

        let config = load(&paths);

        assert!(config.extra.contains_key("wat"));
        assert_eq!(
            config.extra.get("wat").unwrap().as_integer(),
            Some(1)
        );
    }

    #[test]
    fn legacy_config_keys_are_inert_unknown_settings() {
        let config: Config = toml::from_str(
            "default_layer = \"desktop\"\ntray_enabled = false\ndefault_size = [280, 220]\n",
        ).unwrap();
        assert_eq!(config.default_color, "yellow");
        assert_eq!(config.extra["default_layer"].as_str(), Some("desktop"));
        assert_eq!(config.extra["tray_enabled"].as_bool(), Some(false));
        assert_eq!(config.extra["default_size"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn broken_toml_returns_defaults_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_str().unwrap().to_string();
        let waynote_dir = std::path::Path::new(&base).join("waynote");
        std::fs::create_dir_all(&waynote_dir).unwrap();

        let config_file = waynote_dir.join("config.toml");
        std::fs::write(&config_file, "this is not valid toml [[").unwrap();

        let paths = Paths::from_env(|k| match k {
            "XDG_CONFIG_HOME" => Some(base.clone()),
            _ => None,
        });

        let config = load(&paths);

        // Should return all defaults without panicking
        assert_eq!(config.default_color, "yellow");
        assert!(config.extra.is_empty());
    }
}
