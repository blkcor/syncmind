use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use notify::Watcher as _;

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

fn validate_and_canonicalize(path: &PathBuf) -> anyhow::Result<PathBuf> {
    if !path.is_absolute() {
        anyhow::bail!("Path must be absolute: {}", path.display());
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| anyhow::anyhow!("Failed to resolve path {}: {}", path.display(), e))?;
    if !canonical.is_file() {
        anyhow::bail!("Not a file: {}", canonical.display());
    }
    Ok(canonical)
}

async fn run_daemon(foreground: bool) -> anyhow::Result<()> {
    if foreground {
        tracing_subscriber::fmt::init();
    }
    info!("Starting SyncMind daemon...");

    let config = syncmind_core::Config::load()?;
    let db_path = syncmind_core::db_path()?;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let store = Arc::new(syncmind_storage::VectorStore::new(
        &db_path,
        config.embedding_dim,
    )?);
    let embedder = Arc::new(syncmind_rag_engine::embedder::AutoEmbedder::new(&config).await?);

    // Start MCP server.
    let mcp_server = Arc::new(syncmind_mcp_server::McpServer::new(
        store.clone(),
        embedder.clone(),
    ));
    let mcp_transport = config.mcp_transport.clone();
    let bind_addr = config.bind_addr.clone();
    let _mcp_handle = tokio::spawn(async move {
        match mcp_transport {
            syncmind_core::McpTransport::Stdio => {
                if let Err(e) = syncmind_mcp_server::run_stdio_server(mcp_server).await {
                    tracing::error!(error = %e, "stdio server error");
                }
            }
            syncmind_core::McpTransport::Sse => {
                if let Err(e) = syncmind_mcp_server::run_sse_server(mcp_server, &bind_addr).await {
                    tracing::error!(error = %e, "sse server error");
                }
            }
        }
    });

    // Index all registered files on startup.
    let startup_extractor = syncmind_rag_engine::extractor::CompositeExtractor::new();
    for path in &config.registered_files {
        let chunker = syncmind_indexing::chunker_for_path(path, config.chunk_size, config.chunk_overlap);
        if let Err(e) = syncmind_indexing::index_file(path, &startup_extractor, chunker.as_ref(), embedder.as_ref(), &store).await {
            warn!(path = %path.display(), error = %e, "failed to index file on startup");
        } else {
            info!(path = %path.display(), "indexed file");
        }
    }

    // Start file watcher for registered files.
    let (file_tx, file_rx) = tokio::sync::mpsc::channel::<Vec<PathBuf>>(16);
    let mut file_watcher = syncmind_file_watcher::FileWatcher::new(
        config.registered_files.clone(),
        Duration::from_secs(1),
        file_tx,
    )?;

    // Start config file watcher for hot-reload.
    let config_path = syncmind_core::Config::config_path()?;
    let (config_tx, mut config_rx) = tokio::sync::mpsc::channel::<notify::Event>(16);
    let mut config_watcher = notify::RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = config_tx.try_send(event);
            }
        },
        notify::Config::default(),
    )?;
    if config_path.exists() {
        config_watcher.watch(&config_path, notify::RecursiveMode::NonRecursive)?;
    }

    info!("Daemon initialized successfully");

    let indexing_handle = {
        let config = config.clone();
        let store = store.clone();
        let embedder = embedder.clone();
        tokio::spawn(async move {
            if let Err(e) = syncmind_indexing::run_indexing_pipeline(config, store, embedder, file_rx).await {
                warn!(error = %e, "indexing pipeline exited");
            }
        })
    };

    loop {
        tokio::select! {
            Some(_event) = config_rx.recv() => {
                // Debounce config reloads: wait a bit for the file to finish writing.
                tokio::time::sleep(Duration::from_millis(500)).await;

                match syncmind_core::Config::load() {
                    Ok(new_config) => {
                        if let Err(e) = file_watcher.update_paths(&new_config.registered_files) {
                            warn!(error = %e, "failed to update watched paths");
                        } else {
                            info!(
                                count = new_config.registered_files.len(),
                                "config reloaded, updated watched files"
                            );
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to reload config");
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Shutting down...");
                break;
            }
        }
    }

    drop(file_watcher);
    let _ = indexing_handle.await;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon { foreground } => {
            run_daemon(foreground).await?;
        }
        Commands::Register { path } => {
            let canonical = validate_and_canonicalize(&path)?;
            let mut config = syncmind_core::Config::load()?;

            if config.registered_files.contains(&canonical) {
                println!("Already registered: {}", canonical.display());
                return Ok(());
            }

            config.registered_files.push(canonical.clone());
            config.save()?;
            println!("Registered: {}", canonical.display());
        }
        Commands::Unregister { path } => {
            let canonical = validate_and_canonicalize(&path)?;
            let mut config = syncmind_core::Config::load()?;

            let before = config.registered_files.len();
            config.registered_files.retain(|p| p != &canonical);
            let after = config.registered_files.len();

            if after == before {
                println!("Not registered: {}", canonical.display());
                return Ok(());
            }

            config.save()?;
            println!("Unregistered: {}", canonical.display());
        }
        Commands::Status => {
            let config = syncmind_core::Config::load()?;
            let db_path = syncmind_core::db_path()?;
            let store = syncmind_storage::VectorStore::new(&db_path, config.embedding_dim)?;
            let (file_count, chunk_count) = store.get_stats()?;

            println!("SyncMind Status");
            println!("===============");
            println!("Database: {}", db_path.display());
            println!("Registered files: {}", config.registered_files.len());
            println!("Indexed files: {}", file_count);
            println!("Total chunks: {}", chunk_count);
            println!("Ollama URL: {}", config.ollama_url);
            println!("Ollama model: {}", config.ollama_model);
            println!("Embedding dimension: {}", config.embedding_dim);
            println!("Chunk size / overlap: {} / {}", config.chunk_size, config.chunk_overlap);

            for path in &config.registered_files {
                let exists = if path.exists() { "" } else { " [missing]" };
                println!("  - {}{}", path.display(), exists);
            }
        }
        Commands::Search { query, top_k } => {
            use syncmind_rag_engine::embedder::{AutoEmbedder, Embedder};

            let config = syncmind_core::Config::load()?;
            let db_path = syncmind_core::db_path()?;
            let store = syncmind_storage::VectorStore::new(&db_path, config.embedding_dim)?;

            let embedder = AutoEmbedder::new(&config).await?;
            let embeddings = embedder.embed(&[&query]).await?;
            let query_embedding = embeddings.into_iter().next().unwrap();

            let results = store.search(&query_embedding, top_k)?;

            if results.is_empty() {
                println!("No results found.");
                return Ok(());
            }

            println!("Results for '{}' (top_k={}):", query, top_k);
            for (i, result) in results.iter().enumerate() {
                println!(
                    "{}. {} (lines {}-{}, score={:.4})",
                    i + 1,
                    result.file_path.display(),
                    result.start_line,
                    result.end_line,
                    result.score
                );
                // Print first line of content as preview
                let preview = result.content.lines().next().unwrap_or("");
                println!("   {}", preview);
            }
        }
    }

    Ok(())
}
