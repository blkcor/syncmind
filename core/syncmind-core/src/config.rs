use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    #[default]
    Stdio,
    Sse,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogRotation {
    #[default]
    Daily,
    Hourly,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub ollama_url: String,
    pub ollama_model: String,
    pub mcp_transport: McpTransport,
    pub bind_addr: String,
    pub registered_files: Vec<PathBuf>,
    pub embedding_dim: usize,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_to_file")]
    pub log_to_file: bool,
    #[serde(default)]
    pub log_rotation: LogRotation,
    #[serde(default)]
    pub onnx_model_url: Option<String>,
    #[serde(default)]
    pub onnx_tokenizer_url: Option<String>,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_to_file() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "bge-m3".to_string(),
            mcp_transport: McpTransport::Stdio,
            bind_addr: "127.0.0.1:3000".to_string(),
            registered_files: Vec::new(),
            embedding_dim: 1024,
            chunk_size: 512,
            chunk_overlap: 50,
            log_level: default_log_level(),
            log_to_file: default_log_to_file(),
            log_rotation: LogRotation::default(),
            onnx_model_url: None,
            onnx_tokenizer_url: None,
        }
    }
}

impl Config {
    pub fn load() -> Result<Config> {
        let path = Self::config_path()?;

        if path.exists() {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read config file at {}", path.display()))?;
            let config: Config = toml::from_str(&contents)
                .with_context(|| format!("Failed to parse config file at {}", path.display()))?;
            Ok(config)
        } else {
            let config = Config::default();
            config.save()?;
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory at {}", parent.display()))?;
        }

        let contents = toml::to_string_pretty(self)
            .context("Failed to serialize config to TOML")?;

        std::fs::write(&path, contents)
            .with_context(|| format!("Failed to write config file at {}", path.display()))?;

        Ok(())
    }

    pub fn config_path() -> Result<PathBuf> {
        if let Ok(custom) = std::env::var("SYNCMIND_CONFIG_DIR") {
            if !custom.is_empty() {
                return Ok(PathBuf::from(custom).join("config.toml"));
            }
        }
        let config_dir = dirs::config_dir()
            .context("Failed to determine system config directory")?;
        Ok(config_dir.join("syncmind").join("config.toml"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn config_roundtrip() {
        let original = Config {
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "bge-m3".to_string(),
            mcp_transport: McpTransport::Sse,
            bind_addr: "0.0.0.0:8080".to_string(),
            registered_files: vec![PathBuf::from("/tmp/test.md")],
            embedding_dim: 384,
            chunk_size: 256,
            chunk_overlap: 25,
            log_level: "debug".to_string(),
            log_to_file: false,
            log_rotation: LogRotation::Hourly,
            onnx_model_url: Some("https://example.test/model.onnx".to_string()),
            onnx_tokenizer_url: Some("https://example.test/tokenizer.json".to_string()),
        };

        let toml_str = toml::to_string_pretty(&original).unwrap();

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(toml_str.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let contents = std::fs::read_to_string(temp_file.path()).unwrap();
        let deserialized: Config = toml::from_str(&contents).unwrap();

        assert_eq!(deserialized, original);
    }

    #[test]
    fn legacy_config_without_log_fields_uses_defaults() {
        let legacy = r#"
ollama_url = "http://localhost:11434"
ollama_model = "bge-m3"
mcp_transport = "stdio"
bind_addr = "127.0.0.1:3000"
registered_files = []
embedding_dim = 1024
chunk_size = 512
chunk_overlap = 50
"#;
        let parsed: Config = toml::from_str(legacy).unwrap();
        assert_eq!(parsed.log_level, "info");
        assert!(parsed.log_to_file);
        assert_eq!(parsed.log_rotation, LogRotation::Daily);
        assert!(parsed.onnx_model_url.is_none());
    }

    #[test]
    fn default_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("ollama_url"));
        assert!(toml_str.contains("stdio"));
        assert!(toml_str.contains("log_level"));
        assert!(toml_str.contains("log_rotation"));
    }
}
