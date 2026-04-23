use anyhow::Result;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool, Row};
use uuid::Uuid;
use shared::models::CodeSnippet;

pub struct DbService {
    pool: SqlitePool,
}

impl DbService {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url).await?;

        // Run migrations
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS code_snippets (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                language TEXT NOT NULL,
                code TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )"
        ).execute(&pool).await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS job_queue (
                job_id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                status TEXT NOT NULL,
                result TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )"
        ).execute(&pool).await?;

        Ok(Self { pool })
    }

    pub async fn push_job(&self, job_id: Uuid, payload: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO job_queue (job_id, payload, status) VALUES (?, ?, 'pending')"
        )
        .bind(job_id.to_string())
        .bind(payload)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn poll_job(&self) -> Result<Option<(Uuid, String)>> {
        let row = sqlx::query(
            "SELECT job_id, payload FROM job_queue WHERE status = 'pending' LIMIT 1"
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(r) = row {
            let id: String = r.try_get("job_id")?;
            let payload: String = r.try_get("payload")?;
            
            // Mark as processing
            sqlx::query("UPDATE job_queue SET status = 'processing' WHERE job_id = ?")
                .bind(&id)
                .execute(&self.pool)
                .await?;

            Ok(Some((Uuid::parse_str(&id)?, payload)))
        } else {
            Ok(None)
        }
    }

    pub async fn complete_job(&self, job_id: Uuid, result: &str) -> Result<()> {
        sqlx::query(
            "UPDATE job_queue SET status = 'completed', result = ? WHERE job_id = ?"
        )
        .bind(result)
        .bind(job_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_job_result(&self, job_id: Uuid) -> Result<Option<String>> {
        let row = sqlx::query(
            "SELECT result FROM job_queue WHERE job_id = ? AND status = 'completed'"
        )
        .bind(job_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        if let Some(r) = row {
            let res: Option<String> = r.try_get("result")?;
            Ok(res)
        } else {
            Ok(None)
        }
    }

    pub async fn save_snippet(&self, snippet: &CodeSnippet) -> Result<()> {
        let lang = serde_json::to_string(&snippet.language).unwrap();
        sqlx::query(
            "INSERT INTO code_snippets (id, user_id, language, code, created_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(snippet.id.to_string())
        .bind(snippet.user_id.to_string())
        .bind(lang)
        .bind(&snippet.code)
        .bind(snippet.created_at.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_snippet(&self, id: Uuid) -> Result<Option<CodeSnippet>> {
        let id_str = id.to_string();
        let record = sqlx::query(
            "SELECT id, user_id, language, code, created_at FROM code_snippets WHERE id = ?"
        )
        .bind(id_str)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(r) = record {
            let r_language: String = r.try_get("language")?;
            let r_id: String = r.try_get("id")?;
            let r_user_id: String = r.try_get("user_id")?;
            let r_code: String = r.try_get("code")?;
            let r_created_at: String = r.try_get("created_at")?;
            
            let lang: shared::models::Language = serde_json::from_str(&r_language)?;
            Ok(Some(CodeSnippet {
                id: Uuid::parse_str(&r_id)?,
                user_id: Uuid::parse_str(&r_user_id)?,
                language: lang,
                code: r_code,
                created_at: r_created_at.parse().unwrap_or_default(),
            }))
        } else {
            Ok(None)
        }
    }
}
