pub mod config;
pub mod observability;
pub mod paths;
pub mod types;

pub use config::{Config, LogRotation, McpTransport};
pub use observability::init_tracing;
pub use paths::{db_path, local_data_dir, log_dir, model_cache_dir};
pub use types::Chunk;
