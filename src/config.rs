use serde::{Deserialize, Serialize};
use directories::BaseDirs;
use std::fs;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub ignored_folders: Vec<String>,
    #[serde(alias = "max_matches")]
    pub page_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ignored_folders: vec![
                "node_modules".into(),
                "target".into(),
                ".venv".into(),
                ".idea".into(),
                ".vscode".into(),
                ".git".into(),
            ],
            page_size: 10,
        }
    }
}

pub fn load_config() -> Config {
    if let Some(base_dirs) = BaseDirs::new() {
        let config_dir = base_dirs.home_dir().join(".config");

        if !config_dir.exists() {
            let _ = fs::create_dir_all(&config_dir);
        }

        let config_path = config_dir.join("cdx.toml");

        if config_path.exists() {
            if let Ok(contents) = fs::read_to_string(&config_path) {
                if let Ok(config) = toml::from_str(&contents) {
                    return config;
                }
            }
        } else {
            let default_config = Config::default();
            if let Ok(toml_str) = toml::to_string(&default_config) {
                let _ = fs::write(config_path, toml_str);
            }
            return default_config;
        }
    }
    Config::default()
}
