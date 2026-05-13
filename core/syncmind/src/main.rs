use clap::{Parser, Subcommand};
use tracing::info;

#[derive(Parser)]
#[command(name = "syncmind")]
#[command(about = "SyncMind - Local context engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Daemon {
        #[arg(long)]
        foreground: bool,
    },
    Register { path: std::path::PathBuf },
    Unregister { path: std::path::PathBuf },
    Status,
    Search {
        query: String,
        #[arg(long, default_value = "5")]
        top_k: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon { foreground } => {
            if foreground {
                tracing_subscriber::fmt::init();
            }
            info!("Starting SyncMind daemon...");
            let config = syncmind_core::Config::load()?;
            let db_path = syncmind_core::db_path()?;
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _store = syncmind_storage::VectorStore::new(&db_path, config.embedding_dim)?;
            info!("Daemon initialized successfully");
            tokio::signal::ctrl_c().await?;
            info!("Shutting down...");
        }
        Commands::Register { path } => {
            println!("Registering {} (not yet implemented)", path.display());
        }
        Commands::Unregister { path } => {
            println!("Unregistering {} (not yet implemented)", path.display());
        }
        Commands::Status => {
            println!("Status: not yet implemented");
        }
        Commands::Search { query, top_k } => {
            println!(
                "Searching for '{}' (top_k={}) (not yet implemented)",
                query, top_k
            );
        }
    }

    Ok(())
}
