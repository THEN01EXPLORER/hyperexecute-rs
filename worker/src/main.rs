mod execution;
mod queue;

use anyhow::Result;
use deadpool_redis::{Config, Runtime};
use execution::ExecutionEngine;
use queue::QueueService;
use std::sync::Arc;
use tokio::task;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("Starting Worker Node...");

    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());
    let cfg = Config::from_url(redis_url);
    let pool = cfg.create_pool(Some(Runtime::Tokio1)).unwrap();

    // SQLite Setup for fallback
    let db_url = "sqlite:snippets.db?mode=rwc";
    let sqlite_pool = sqlx::SqlitePool::connect(db_url).await?;

    let queue_service = Arc::new(QueueService::new(pool).with_sqlite(sqlite_pool));
    let execution_engine = Arc::new(match ExecutionEngine::new().await {
        Ok(engine) => engine,
        Err(e) => {
            tracing::warn!("Docker unavailable: {}. Using local execution.", e);
            ExecutionEngine::new_local()
        }
    });

    // Concurrency limit: 10 concurrent jobs per worker node
    let semaphore = Arc::new(tokio::sync::Semaphore::new(10));

    loop {
        match queue_service.pop_job().await {
            Ok(Some(job)) => {
                let permit = semaphore.clone().acquire_owned().await.unwrap();
                let queue_clone = queue_service.clone();
                let exec_clone = execution_engine.clone();

                task::spawn(async move {
                    tracing::info!("Executing job: {}", job.job_id);
                    match exec_clone.execute_job(job.clone()).await {
                        Ok(result) => {
                            if let Err(e) = queue_clone.push_result(result).await {
                                tracing::error!("Failed to push job result: {}", e);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Execution failed internally: {}", e);
                            // Fallback result string
                            let result = shared::models::ExecutionResult {
                                job_id: job.job_id,
                                stdout: "".to_string(),
                                stderr: "".to_string(),
                                exit_code: -1,
                                time_taken_ms: 0,
                                error: Some(format!("Internal Execute Error: {}", e)),
                            };
                            let _ = queue_clone.push_result(result).await;
                        }
                    }
                    drop(permit);
                });
            }
            Ok(None) => {
                // Timeout, no job, continue polling
            }
            Err(e) => {
                tracing::error!("Failed to pop job from queue: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }
}
