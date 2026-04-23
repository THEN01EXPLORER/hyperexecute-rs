use anyhow::{Context, Result};
use deadpool_redis::{redis::AsyncCommands, Pool};
use shared::models::{ExecutionJob, ExecutionResult};
use sqlx::{SqlitePool, Row};
use uuid::Uuid;

pub struct QueueService {
    pool: Pool,
    sqlite_pool: Option<SqlitePool>,
}

impl QueueService {
    pub fn new(pool: Pool) -> Self {
        Self { pool, sqlite_pool: None }
    }

    pub fn with_sqlite(mut self, sqlite_pool: SqlitePool) -> Self {
        self.sqlite_pool = Some(sqlite_pool);
        self
    }

    pub async fn pop_job(&self) -> Result<Option<ExecutionJob>> {
        // Try Redis first
        if let Ok(mut conn) = self.pool.get().await {
            // Blocking pop from the 'execution_jobs' list, wait up to 5 seconds
            if let Ok(result) = conn.brpop::<_, Option<(String, String)>>("execution_jobs", 5.0).await {
                if let Some((_, job_json)) = result {
                    let job: ExecutionJob = serde_json::from_str(&job_json)?;
                    return Ok(Some(job));
                }
            }
        }
        
        // Fallback to SQLite
        if let Some(pool) = &self.sqlite_pool {
            let row = sqlx::query(
                "SELECT job_id, payload FROM job_queue WHERE status = 'pending' LIMIT 1"
            )
            .fetch_optional(pool)
            .await?;

            if let Some(r) = row {
                let id: String = r.try_get("job_id")?;
                let payload: String = r.try_get("payload")?;
                
                // Mark as processing
                sqlx::query("UPDATE job_queue SET status = 'processing' WHERE job_id = ?")
                    .bind(&id)
                    .execute(pool)
                    .await?;

                let job: ExecutionJob = serde_json::from_str(&payload)?;
                return Ok(Some(job));
            }
        }

        Ok(None)
    }

    pub async fn push_result(&self, result: ExecutionResult) -> Result<()> {
        let job_id = result.job_id;
        let result_json = serde_json::to_string(&result)?;
        
        // Try Redis
        if let Ok(mut conn) = self.pool.get().await {
            // Publish to pub/sub (for immediate websocket delivery)
            let channel = format!("job_result:{}", job_id);
            let _: Result<(), _> = conn.publish(channel, &result_json).await;

            // Optional: Save to a hash for polling backup
            let _: Result<(), _> = conn.hset("execution_results", job_id.to_string(), &result_json).await;
        }

        // Always update SQLite if available
        if let Some(pool) = &self.sqlite_pool {
            sqlx::query(
                "UPDATE job_queue SET status = 'completed', result = ? WHERE job_id = ?"
            )
            .bind(&result_json)
            .bind(job_id.to_string())
            .execute(pool)
            .await?;
        }

        Ok(())
    }
}
