use axum::{
    extract::{State, Path, ws::{WebSocket, WebSocketUpgrade, Message}},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use shared::models::{ExecutionJob, Language};
use std::sync::Arc;
use uuid::Uuid;
use crate::queue::QueueService;
use crate::db::DbService;

#[derive(Clone)]
pub struct AppState {
    pub queue_service: Arc<QueueService>,
    pub db_service: Arc<DbService>,
}


#[derive(Deserialize)]
pub struct ExecuteRequest {
    language: Language,
    code: String,
    input: Option<String>,
}

#[derive(Serialize)]
pub struct ExecuteResponse {
    job_id: Uuid,
    stdout: String,
    stderr: String,
    exit_code: i64,
    time_taken_ms: u64,
    error: Option<String>,
}

pub async fn execute_code(
    State(state): State<AppState>,
    Json(payload): Json<ExecuteRequest>,
) -> impl IntoResponse {
    let job_id = Uuid::new_v4();
    let job = ExecutionJob {
        job_id,
        language: payload.language,
        code: payload.code,
        user_id: None,
        stdin: payload.input,
    };

    match state.queue_service.submit_and_wait(job).await {
        Ok(result) => {
            let res = ExecuteResponse {
                job_id: result.job_id,
                stdout: result.stdout,
                stderr: result.stderr,
                exit_code: result.exit_code,
                time_taken_ms: result.time_taken_ms,
                error: result.error,
            };
            (StatusCode::OK, Json(res))
        }
        Err(e) => {
            let res = ExecuteResponse {
                job_id,
                stdout: "".to_string(),
                stderr: "".to_string(),
                exit_code: -1,
                time_taken_ms: 0,
                error: Some(format!("Failed to submit job: {}", e)),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(res))
        }
    }
}

#[derive(Deserialize)]
pub struct SaveRequest {
    language: Language,
    code: String,
    input: Option<String>,
}

#[derive(Serialize)]
pub struct SaveResponse {
    snippet_id: Uuid,
}

pub async fn save_code(
    State(state): State<AppState>,
    Json(payload): Json<SaveRequest>,
) -> impl IntoResponse {
    let snippet = shared::models::CodeSnippet {
        id: Uuid::new_v4(),
        user_id: Uuid::nil(), // Anonymous user for now
        language: payload.language,
        code: payload.code,
        stdin: payload.input,
        created_at: chrono::Utc::now(),
    };

    match state.db_service.save_snippet(&snippet).await {
        Ok(_) => {
            (StatusCode::OK, Json(SaveResponse { snippet_id: snippet.id }))
        }
        Err(e) => {
            tracing::error!("Failed to save snippet: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(SaveResponse { snippet_id: Uuid::nil() }))
        }
    }
}

pub async fn load_code(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.db_service.get_snippet(id).await {
        Ok(Some(snippet)) => {
            (StatusCode::OK, Json(Some(snippet)))
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(None)),
        Err(e) => {
            tracing::error!("Failed to load snippet: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(None))
        }
    }
}

pub async fn ws_execute(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_execution(socket, state))
}

async fn handle_ws_execution(mut socket: WebSocket, state: AppState) {
    if let Some(msg) = socket.recv().await {
        if let Ok(Message::Text(text)) = msg {
            if let Ok(payload) = serde_json::from_str::<ExecuteRequest>(&text) {
                let job = ExecutionJob {
                    job_id: Uuid::new_v4(),
                    language: payload.language,
                    code: payload.code,
                    user_id: None,
                    stdin: None,
                };
                let _ = state.queue_service.submit_and_stream(job, socket).await;
            }
        }
    }
}
