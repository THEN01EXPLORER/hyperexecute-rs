use anyhow::{Context, Result};
use bollard::container::{
    Config, CreateContainerOptions, LogOutput, LogsOptions, RemoveContainerOptions,
    StartContainerOptions, WaitContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bollard::Docker;
use futures_util::stream::StreamExt;
use shared::models::{ExecutionJob, ExecutionResult};
use std::time::Instant;

const MAX_EXECUTION_TIME_MS: u64 = 5000; // 5 seconds
const MEMORY_LIMIT_BYTES: i64 = 128 * 1024 * 1024; // 128 MB

pub struct ExecutionEngine {
    docker: Option<Docker>,
}

impl ExecutionEngine {
    pub async fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to local Docker daemon")?;
        // Actually verify the daemon is running by pinging it
        docker.ping().await.context("Docker daemon is not responding")?;
        tracing::info!("Docker daemon connected successfully");
        Ok(Self { docker: Some(docker) })
    }

    pub fn new_local() -> Self {
        tracing::info!("Using local process execution (no Docker)");
        Self { docker: None }
    }

    // Removed redundant pull_image

    pub async fn execute_job(&self, job: ExecutionJob) -> Result<ExecutionResult> {
        if let Some(docker) = &self.docker {
            self.execute_docker_job(docker, job).await
        } else {
            self.execute_local_job(job).await
        }
    }

    async fn execute_local_job(&self, job: ExecutionJob) -> Result<ExecutionResult> {
        let start_time = Instant::now();
        let filename = format!("temp_{}.{}", job.job_id, job.language.file_extension());
        
        // Write code to temp file
        tokio::fs::write(&filename, &job.code).await?;

        let mut cmd_parts = job.language.execution_cmd(&filename);
        let program = cmd_parts.remove(0);
        
        // Handle C++ compilation if needed
        let (actual_program, actual_args) = if job.language.file_extension() == "cpp" {
             // For C++, the command is like ["g++", "-O3", "main.cpp", "-o", "prog", "&&", "./prog"]
             // This is meant for a shell. Locally we should compile then run.
             let compile_status = std::process::Command::new("g++")
                .arg("-O3")
                .arg(&filename)
                .arg("-o")
                .arg(format!("temp_{}.exe", job.job_id))
                .status()?;
             
             if !compile_status.success() {
                 return Ok(ExecutionResult {
                     job_id: job.job_id,
                     stdout: "".to_string(),
                     stderr: "Compilation failed".to_string(),
                     exit_code: -1,
                     time_taken_ms: start_time.elapsed().as_millis() as u64,
                     error: Some("Compilation Error".to_string()),
                 });
             }
             (format!("./temp_{}.exe", job.job_id), vec![])
        } else {
            (program, cmd_parts)
        };

        let output = tokio::process::Command::new(actual_program)
            .args(actual_args)
            .output()
            .await?;

        let time_taken_ms = start_time.elapsed().as_millis() as u64;

        // Cleanup
        let _ = tokio::fs::remove_file(&filename).await;
        if job.language.file_extension() == "cpp" {
            let _ = tokio::fs::remove_file(format!("temp_{}.exe", job.job_id)).await;
        }

        Ok(ExecutionResult {
            job_id: job.job_id,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1) as i64,
            time_taken_ms,
            error: None,
        })
    }

    async fn execute_docker_job(&self, docker: &Docker, job: ExecutionJob) -> Result<ExecutionResult> {
        let image = job.language.docker_image();
        
        // Ensure image exists locally before running
        let _ = self.pull_image_internal(docker, image).await; 

        let filename = format!("main.{}", job.language.file_extension());
        let cmd = job.language.execution_cmd(&filename);
        let container_name = format!("exec_{}", job.job_id);

        // Security constraints
        let host_config = HostConfig {
            memory: Some(MEMORY_LIMIT_BYTES),
            memory_swap: Some(MEMORY_LIMIT_BYTES), // No swap
            nano_cpus: Some(1_000_000_000),        // 1 CPU core
            network_mode: Some("none".to_string()),
            pids_limit: Some(64),
            ..Default::default()
        };

        // Create a script that creates the file and runs the command
        let escaped_code = job.code.replace("'", "'\\''");
        let shell_cmd = format!("echo '{}' > {} && {}", escaped_code, filename, cmd.join(" "));

        let config = Config {
            image: Some(image),
            cmd: Some(vec!["sh", "-c", &shell_cmd]),
            host_config: Some(host_config),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let container = docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name.as_str(),
                    platform: None,
                }),
                config,
            )
            .await
            .context("Failed to create container")?;

        let start_time = Instant::now();

        docker
            .start_container(&container.id, None::<StartContainerOptions<String>>)
            .await
            .context("Failed to start container")?;

        // Wait for container to exit, with timeout
        let wait_result = docker.wait_container(
            &container.id,
            Some(WaitContainerOptions {
                condition: "not-running",
            }),
        ).next().await;

        let time_taken_ms = start_time.elapsed().as_millis() as u64;

        let mut exit_code = -1;
        let mut error_msg = None;

        if time_taken_ms > MAX_EXECUTION_TIME_MS {
             error_msg = Some("Execution timed out".to_string());
             // Container will be force removed below
        } else if let Some(Ok(res)) = wait_result {
             exit_code = res.status_code;
        } else {
             error_msg = Some("Execution failed or killed".to_string());
        }

        // Fetch logs
        let mut logs_stream = docker.logs::<String>(
            &container.id,
            Some(LogsOptions {
                stdout: true,
                stderr: true,
                ..Default::default()
            }),
        );

        let mut stdout = String::new();
        let mut stderr = String::new();

        while let Some(log_res) = logs_stream.next().await {
            if let Ok(log) = log_res {
                match log {
                    LogOutput::StdOut { message } => {
                        stdout.push_str(&String::from_utf8_lossy(&message));
                    }
                    LogOutput::StdErr { message } => {
                        stderr.push_str(&String::from_utf8_lossy(&message));
                    }
                    _ => {}
                }
            }
        }

        // Cleanup
        docker
            .remove_container(
                &container.id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .context("Failed to remove container")?;

        Ok(ExecutionResult {
            job_id: job.job_id,
            stdout,
            stderr,
            exit_code,
            time_taken_ms,
            error: error_msg,
        })
    }

    async fn pull_image_internal(&self, docker: &Docker, image: &str) -> Result<()> {
        let mut stream = docker.create_image(
            Some(CreateImageOptions {
                from_image: image,
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(res) = stream.next().await {
            res?;
        }
        Ok(())
    }
}
