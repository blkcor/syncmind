pub mod config;
pub mod paths;

pub use config::{Config, McpTransport};
pub use paths::{data_dir, db_path, log_dir, model_cache_dir};
