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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OcrMode {
    Disabled,
    #[default]
    Auto,
    Force,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpineConfig {
    /// Spine sync gateway URL (e.g. `https://spine.example.com` or `http://192.168.1.10:8080`).
    /// `None` disables the desktop sync subsystem entirely. See PRD 004 §US-020.
    #[serde(default)]
    pub url: Option<String>,

    /// Optional PEM file containing a self-signed CA certificate that the HTTP client should
    /// trust in addition to the system roots. Per PRD 004 §US-020, plain HTTP and IP-host
    /// URLs are allowed; users opting into HTTPS with a private CA point this at the PEM.
    #[serde(default)]
    pub trust_ca_path: Option<PathBuf>,

    /// SHA-256 hex fingerprint (64 chars) of the paired peer's Ed25519 public key.
    /// `None` when no pairing has completed. See PRD 004 §US-022.
    #[serde(default)]
    pub paired_peer_fingerprint: Option<String>,

    /// Self-reported device type of the paired peer (`"desktop"` or `"mobile"`).
    #[serde(default)]
    pub paired_peer_device_type: Option<String>,

    /// RFC3339 timestamp at which the current pairing completed (UTC).
    #[serde(default)]
    pub paired_at: Option<String>,

    /// UUIDv4 (as a string) of the paired peer's `devices.id` on the Spine. Cached locally
    /// so the desktop can render it in the UI without an extra `/me` round-trip. See PRD 004
    /// §US-022 and the OpenSpec change `desktop-spine-client`.
    #[serde(default)]
    pub peer_device_id_uuid: Option<String>,
}

impl SpineConfig {
    /// Returns true when the desktop sync subsystem should be active (URL is set).
    pub fn is_enabled(&self) -> bool {
        self.url.as_deref().is_some_and(|s| !s.is_empty())
    }

    /// Returns true when the configured URL uses plain HTTP (used by the desktop UI to render
    /// a non-blocking warning banner per PRD 004 §US-020). Returns false when no URL is set
    /// or the URL is malformed.
    pub fn is_plain_http(&self) -> bool {
        self.url
            .as_deref()
            .map(|s| s.starts_with("http://"))
            .unwrap_or(false)
    }

    /// Returns true when a paired peer is currently recorded.
    pub fn is_paired(&self) -> bool {
        self.paired_peer_fingerprint.is_some()
    }

    /// Clears every `paired_*` field. Called by the desktop's `spine_unpair` command.
    pub fn clear_pairing(&mut self) {
        self.paired_peer_fingerprint = None;
        self.paired_peer_device_type = None;
        self.paired_at = None;
        self.peer_device_id_uuid = None;
    }
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
    #[serde(default)]
    pub hybrid_search_enabled: bool,
    #[serde(default)]
    pub relevance_threshold: Option<f64>,
    #[serde(default)]
    pub reranker_enabled: bool,
    #[serde(default)]
    pub reranker_model_path: Option<String>,
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
    #[serde(default)]
    pub ocr_mode: OcrMode,
    #[serde(default = "default_pdf_text_quality_threshold")]
    pub pdf_text_quality_threshold: f64,
    #[serde(default)]
    pub ocr_binary_path: Option<String>,
    #[serde(default)]
    pub pdf_renderer_path: Option<String>,
    #[serde(default)]
    pub spine: SpineConfig,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_to_file() -> bool {
    true
}

fn default_pdf_text_quality_threshold() -> f64 {
    0.35
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
            hybrid_search_enabled: false,
            relevance_threshold: None,
            reranker_enabled: false,
            reranker_model_path: None,
            log_level: default_log_level(),
            log_to_file: default_log_to_file(),
            log_rotation: LogRotation::default(),
            onnx_model_url: None,
            onnx_tokenizer_url: None,
            ocr_mode: OcrMode::default(),
            pdf_text_quality_threshold: default_pdf_text_quality_threshold(),
            ocr_binary_path: None,
            pdf_renderer_path: None,
            spine: SpineConfig::default(),
        }
    }
}

impl Config {
    pub fn expected_embedding_dim_for_model(model: &str) -> Option<usize> {
        match model.trim().to_ascii_lowercase().as_str() {
            "bge-m3" => Some(1024),
            "bge-small" | "bge-small-en-v1.5" => Some(384),
            _ => None,
        }
    }

    pub fn normalize_embedding_dim(&mut self) {
        if let Some(expected) = Self::expected_embedding_dim_for_model(&self.ollama_model) {
            self.embedding_dim = expected;
        }
    }

    pub fn validate_ocr_config(&self) -> Result<()> {
        if !(0.0..=1.0).contains(&self.pdf_text_quality_threshold) {
            anyhow::bail!(
                "pdf_text_quality_threshold must be between 0.0 and 1.0, got {}",
                self.pdf_text_quality_threshold
            );
        }
        Ok(())
    }

    pub fn load() -> Result<Config> {
        let path = Self::config_path()?;

        if path.exists() {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read config file at {}", path.display()))?;
            let mut config: Config = toml::from_str(&contents)
                .with_context(|| format!("Failed to parse config file at {}", path.display()))?;
            config.normalize_embedding_dim();
            config.validate_ocr_config()?;
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
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory at {}", parent.display())
            })?;
        }

        let contents =
            toml::to_string_pretty(self).context("Failed to serialize config to TOML")?;

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
        let config_dir =
            dirs::config_dir().context("Failed to determine system config directory")?;
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
            hybrid_search_enabled: true,
            relevance_threshold: Some(0.75),
            reranker_enabled: true,
            reranker_model_path: Some("/tmp/reranker.onnx".to_string()),
            log_level: "debug".to_string(),
            log_to_file: false,
            log_rotation: LogRotation::Hourly,
            onnx_model_url: Some("https://example.test/model.onnx".to_string()),
            onnx_tokenizer_url: Some("https://example.test/tokenizer.json".to_string()),
            ocr_mode: OcrMode::Force,
            pdf_text_quality_threshold: 0.5,
            ocr_binary_path: Some("/usr/local/bin/tesseract".to_string()),
            pdf_renderer_path: Some("/usr/local/bin/pdftoppm".to_string()),
            spine: SpineConfig::default(),
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
        assert_eq!(parsed.ocr_mode, OcrMode::Auto);
        assert_eq!(parsed.pdf_text_quality_threshold, 0.35);
    }

    #[test]
    fn default_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("ollama_url"));
        assert!(toml_str.contains("stdio"));
        assert!(toml_str.contains("log_level"));
        assert!(toml_str.contains("log_rotation"));
        assert!(toml_str.contains("ocr_mode"));
        assert!(toml_str.contains("pdf_text_quality_threshold"));
    }

    #[test]
    fn normalize_embedding_dim_uses_known_model_defaults() {
        let mut config = Config {
            ollama_model: "bge-small".to_string(),
            embedding_dim: 1024,
            ..Config::default()
        };

        config.normalize_embedding_dim();

        assert_eq!(config.embedding_dim, 384);
    }

    #[test]
    fn normalize_embedding_dim_preserves_custom_model_dim() {
        let mut config = Config {
            ollama_model: "custom-embedder".to_string(),
            embedding_dim: 768,
            ..Config::default()
        };

        config.normalize_embedding_dim();

        assert_eq!(config.embedding_dim, 768);
    }

    #[test]
    fn invalid_ocr_threshold_is_rejected() {
        let config = Config {
            pdf_text_quality_threshold: 1.5,
            ..Config::default()
        };

        let result = config.validate_ocr_config();

        assert!(result.is_err());
    }

    #[test]
    fn legacy_config_without_spine_section_defaults_to_empty() {
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
        assert!(!parsed.spine.is_enabled());
        assert!(!parsed.spine.is_paired());
        assert!(parsed.spine.url.is_none());
        assert!(parsed.spine.trust_ca_path.is_none());
        assert!(parsed.spine.paired_peer_fingerprint.is_none());
    }

    #[test]
    fn spine_section_roundtrips() {
        let original = Config {
            spine: SpineConfig {
                url: Some("https://spine.example.com".to_string()),
                trust_ca_path: Some(PathBuf::from("/etc/ssl/spine-ca.pem")),
                paired_peer_fingerprint: Some("a".repeat(64)),
                paired_peer_device_type: Some("mobile".to_string()),
                paired_at: Some("2026-05-24T03:00:00Z".to_string()),
                peer_device_id_uuid: Some("9c4a3b8e-1d2f-4a3b-9c4a-3b8e1d2f4a3b".to_string()),
            },
            ..Config::default()
        };

        let serialized = toml::to_string_pretty(&original).unwrap();
        let parsed: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn spine_is_plain_http_detects_http_scheme() {
        let mut cfg = SpineConfig::default();
        assert!(!cfg.is_plain_http());

        cfg.url = Some("http://192.168.1.10:8080".to_string());
        assert!(cfg.is_plain_http());

        cfg.url = Some("https://spine.example.com".to_string());
        assert!(!cfg.is_plain_http());
    }

    #[test]
    fn spine_clear_pairing_resets_all_paired_fields() {
        let mut cfg = SpineConfig {
            url: Some("https://spine.example.com".to_string()),
            trust_ca_path: None,
            paired_peer_fingerprint: Some("abc".to_string()),
            paired_peer_device_type: Some("mobile".to_string()),
            paired_at: Some("2026-05-24T03:00:00Z".to_string()),
            peer_device_id_uuid: Some("uuid-here".to_string()),
        };

        cfg.clear_pairing();

        assert_eq!(cfg.url, Some("https://spine.example.com".to_string()));
        assert!(!cfg.is_paired());
        assert!(cfg.paired_peer_fingerprint.is_none());
        assert!(cfg.paired_peer_device_type.is_none());
        assert!(cfg.paired_at.is_none());
        assert!(cfg.peer_device_id_uuid.is_none());
    }
}
