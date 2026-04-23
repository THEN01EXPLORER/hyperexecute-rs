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
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:snippets.db?mode=rwc".to_string());
    let db_service = Arc::new(db::DbService::new(&db_url).await?);
    
    // In-process execution (no Redis/Worker needed)
    let queue_service = Arc::new(queue::QueueService::new());
    
    let state = api::AppState { queue_service, db_service };

    let app = Router::new()
        .route("/execute", post(api::execute_code))
        .route("/ws/execute", get(api::ws_execute))
        .route("/save", post(api::save_code))
        .route("/load/:id", get(api::load_code))
        .route("/health", get(|| async { "OK" }))
        .fallback_service(ServeDir::new("server/static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", listener.local_addr()?);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
