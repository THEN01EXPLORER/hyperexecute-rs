use anyhow::Result;
use shared::models::{ExecutionJob, ExecutionResult};
use std::time::Instant;

pub struct QueueService;

impl QueueService {
    pub fn new() -> Self {
        Self
    }

    /// Execute code directly in-process using local system interpreters.
    /// No Redis, no Docker, no separate worker needed.
    pub async fn submit_and_wait(&self, job: ExecutionJob) -> Result<ExecutionResult> {
        tracing::info!("Executing job {} locally", job.job_id);
        let start_time = Instant::now();

        let ext = job.language.file_extension();
        let filename = format!("temp_{}.{}", job.job_id, ext);

        // Write code to temp file
        tokio::fs::write(&filename, &job.code).await?;

        let result = if ext == "cpp" {
            // C++: compile then run
            self.execute_cpp(&filename, &job, start_time).await
        } else {
            // Python / JavaScript: run directly
            let cmd_parts = job.language.execution_cmd(&filename);
            self.execute_interpreted(&cmd_parts, &job, start_time).await
        };

        // Cleanup temp files
        let _ = tokio::fs::remove_file(&filename).await;
        if ext == "cpp" {
            let _ = tokio::fs::remove_file(format!("temp_{}.exe", job.job_id)).await;
        }

        result
    }

    async fn execute_interpreted(
        &self,
        cmd_parts: &[String],
        job: &ExecutionJob,
        start_time: Instant,
    ) -> Result<ExecutionResult> {
        let program = &cmd_parts[0];
        let args = &cmd_parts[1..];

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::process::Command::new(program)
                .args(args)
                .output(),
        )
        .await;

        let time_taken_ms = start_time.elapsed().as_millis() as u64;

        match output {
            Ok(Ok(output)) => Ok(ExecutionResult {
                job_id: job.job_id,
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code().unwrap_or(-1) as i64,
                time_taken_ms,
                error: None,
            }),
            Ok(Err(e)) => Ok(ExecutionResult {
                job_id: job.job_id,
                stdout: "".to_string(),
                stderr: format!("Failed to run: {}", e),
                exit_code: -1,
                time_taken_ms,
                error: Some(format!("Process error: {}", e)),
            }),
            Err(_) => Ok(ExecutionResult {
                job_id: job.job_id,
                stdout: "".to_string(),
                stderr: "".to_string(),
                exit_code: -1,
                time_taken_ms,
                error: Some("Execution timed out (10s limit)".to_string()),
            }),
        }
    }

    async fn execute_cpp(
        &self,
        filename: &str,
        job: &ExecutionJob,
        start_time: Instant,
    ) -> Result<ExecutionResult> {
        let exe_name = format!("temp_{}.exe", job.job_id);

        // Compile
        let compile = tokio::process::Command::new("g++")
            .args(&["-O2", filename, "-o", &exe_name])
            .output()
            .await?;

        if !compile.status.success() {
            return Ok(ExecutionResult {
                job_id: job.job_id,
                stdout: "".to_string(),
                stderr: String::from_utf8_lossy(&compile.stderr).to_string(),
                exit_code: -1,
                time_taken_ms: start_time.elapsed().as_millis() as u64,
                error: Some("Compilation failed".to_string()),
            });
        }

        // Run the compiled binary
        let cmd_parts = vec![format!("./{}", exe_name)];
        self.execute_interpreted(&cmd_parts, job, start_time).await
    }
}
