use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Language {
    Python,
    Cpp,
    JavaScript,
}

impl Language {
    pub fn docker_image(&self) -> &'static str {
        match self {
            Language::Python => "python:3.10-alpine",
            Language::Cpp => "gcc:12",
            Language::JavaScript => "node:18-alpine",
        }
    }

    pub fn file_extension(&self) -> &'static str {
        match self {
            Language::Python => "py",
            Language::Cpp => "cpp",
            Language::JavaScript => "js",
        }
    }

    pub fn execution_cmd(&self, filename: &str) -> Vec<String> {
        match self {
            Language::Python => vec!["python".to_string(), filename.to_string()],
            Language::Cpp => vec![
                "g++".to_string(),
                "-O3".to_string(),
                filename.to_string(),
                "-o".to_string(),
                "prog".to_string(),
                "&&".to_string(),
                "./prog".to_string(),
            ],
            Language::JavaScript => vec!["node".to_string(), filename.to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionJob {
    pub job_id: Uuid,
    pub language: Language,
    pub code: String,
    pub user_id: Option<Uuid>, 
    pub stdin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub job_id: Uuid,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i64,
    pub time_taken_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSnippet {
    pub id: Uuid,
    pub user_id: Uuid,
    pub language: Language,
    pub code: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
