use std::fs;
use std::path::PathBuf;

use color_eyre::Result;
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Preferred device ID
    #[serde(default)]
    pub default_device: Option<String>,

    /// Custom group names: thread_id → display name
    #[serde(default)]
    pub group_names: std::collections::HashMap<String, String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            debug!("No config file at {:?}, using defaults", path);
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        debug!("Loaded config from {:?}", path);
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        debug!("Saved config to {:?}", path);
        Ok(())
    }

    fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("kdeconnect-sms-tui")
            .join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.default_device.is_none());
        assert!(config.group_names.is_empty());
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut config = Config::default();
        config.default_device = Some("abc123".into());
        config.group_names.insert("42".into(), "Family Chat".into());

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.default_device, Some("abc123".into()));
        assert_eq!(
            deserialized.group_names.get("42"),
            Some(&"Family Chat".to_string())
        );
    }

    #[test]
    fn test_save_and_load() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        let mut config = Config::default();
        config.default_device = Some("test123".into());

        let content = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&path, content).unwrap();

        let loaded_content = std::fs::read_to_string(&path).unwrap();
        let loaded: Config = toml::from_str(&loaded_content).unwrap();
        assert_eq!(loaded.default_device, Some("test123".into()));
    }
}
