mod api;
mod queue;
mod db;

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::services::ServeDir;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("Starting HyperExecute Server...");

    // SQLite Setup
    let db_url = "sqlite:snippets.db?mode=rwc";
    let db_service = Arc::new(db::DbService::new(db_url).await?);
    
    // In-process execution (no Redis/Worker needed)
    let queue_service = Arc::new(queue::QueueService::new());
    
    let state = api::AppState { queue_service, db_service };

    let app = Router::new()
        .route("/execute", post(api::execute_code))
        .route("/save", post(api::save_code))
        .route("/load/:id", get(api::load_code))
        .route("/health", get(|| async { "OK" }))
        .fallback_service(ServeDir::new("server/static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("Listening on {}", listener.local_addr()?);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
