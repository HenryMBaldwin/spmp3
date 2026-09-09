use std::path::{Path, PathBuf};

use axum::{
    Json,
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use common::{
    manifest::{MANIFEST_FILE, Manifest},
    overrides::{Action, OVERRIDES_FILE, Override, Overrides, validate_url},
    status::{STATUS_FILE, Status, now},
};
use mp3sync::{DeviceFile, DeviceState, plan};
use serde::{Deserialize, Serialize};
use tokio_util::io::ReaderStream;

use crate::{config::Config, error::WebError};

#[derive(Debug, Deserialize)]
pub struct PlanRequest {
    pub contents: Vec<DeviceFile>,
}

#[derive(Debug, Serialize)]
pub struct CopyStep {
    pub id: String,
    pub url: String,
    pub to: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct RenameStep {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct PlanResponse {
    pub copy: Vec<CopyStep>,
    pub rename: Vec<RenameStep>,
    pub delete: Vec<PathBuf>,
}

fn manifest(config: &Config) -> Result<Manifest, WebError> {
    Ok(Manifest::load(&config.library_dir.join(MANIFEST_FILE))?)
}

pub async fn plan_handler(
    State(config): State<Config>,
    Json(request): Json<PlanRequest>,
) -> Result<Json<PlanResponse>, WebError> {
    let manifest = manifest(&config)?;
    let (state, orphans) = DeviceState::from_contents(&manifest, &request.contents);
    let mut steps = plan(&manifest, &state);
    steps.delete.extend(orphans);

    tracing::info!(
        reported = request.contents.len(),
        copy = steps.copy.len(),
        rename = steps.rename.len(),
        delete = steps.delete.len(),
        "planned device sync"
    );

    Ok(Json(PlanResponse {
        copy: steps
            .copy
            .into_iter()
            .map(|step| CopyStep {
                url: format!("/api/track/{}", step.id),
                id: step.id,
                to: step.to,
            })
            .collect(),
        rename: steps
            .rename
            .into_iter()
            .map(|step| RenameStep {
                from: step.from,
                to: step.to,
            })
            .collect(),
        delete: steps.delete,
    }))
}

fn is_inside(root: &Path, candidate: &Path) -> bool {
    match (root.canonicalize(), candidate.canonicalize()) {
        (Ok(root), Ok(candidate)) => candidate.starts_with(root),
        _ => false,
    }
}

pub async fn track_handler(
    State(config): State<Config>,
    AxumPath(id): AxumPath<String>,
) -> Result<Response, WebError> {
    let manifest = manifest(&config)?;
    let entry = manifest
        .entries
        .get(&id)
        .ok_or_else(|| WebError::UnknownTrack { id: id.clone() })?;

    let path = config.library_dir.join(&entry.path);
    if !is_inside(&config.library_dir, &path) {
        return Err(WebError::UnknownTrack { id });
    }

    let file = tokio::fs::File::open(&path).await?;
    let len = file.metadata().await?.len();
    let stream = ReaderStream::new(file);

    let mut response = Response::new(Body::from_stream(stream));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/mpeg"));
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&len.to_string()).unwrap_or_else(|_| HeaderValue::from_static("0")),
    );

    Ok(response.into_response())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum OverrideRequest {
    Ignore {
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        artist: String,
        #[serde(default)]
        title: String,
    },
    Source {
        id: String,
        url: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        artist: String,
        #[serde(default)]
        title: String,
    },
    Clear {
        id: String,
    },
}

pub async fn overrides_handler(State(config): State<Config>) -> Result<Json<Overrides>, WebError> {
    Ok(Json(Overrides::load(
        &config.library_dir.join(OVERRIDES_FILE),
    )?))
}

pub async fn set_override_handler(
    State(config): State<Config>,
    Json(request): Json<OverrideRequest>,
) -> Result<Json<Overrides>, WebError> {
    let path = config.library_dir.join(OVERRIDES_FILE);
    let mut overrides = Overrides::load(&path)?;

    let (id, label, artist, title, action) = match request {
        OverrideRequest::Ignore {
            id,
            label,
            artist,
            title,
        } => (id, label, artist, title, Some(Action::Ignore)),
        OverrideRequest::Source {
            id,
            url,
            label,
            artist,
            title,
        } => {
            validate_url(&url)?;
            (id, label, artist, title, Some(Action::Source { url }))
        }
        OverrideRequest::Clear { id } => (id, String::new(), String::new(), String::new(), None),
    };

    if let Some(action) = action {
        tracing::info!(id = %id, ?action, "recorded override");
        overrides.entries.insert(
            id,
            Override {
                action,
                label,
                artist,
                title,
                at: now(),
            },
        );
    } else {
        tracing::info!(id = %id, "cleared override");
        overrides.entries.remove(&id);
    }

    overrides.save(&path)?;

    Ok(Json(overrides))
}

pub async fn status_handler(State(config): State<Config>) -> Result<Json<Status>, WebError> {
    Ok(Json(Status::load(&config.library_dir.join(STATUS_FILE))?))
}

pub async fn index_handler() -> impl IntoResponse {
    axum::response::Html(include_str!("../static/index.html"))
}
