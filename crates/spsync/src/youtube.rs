use std::{process::Stdio, time::Duration};

use common::overrides::validate_url;

use crate::error::SpsyncError;

const TIMEOUT: Duration = Duration::from_mins(5);
const MAX_BYTES: usize = 128 * 1024 * 1024;
const FORMAT: &str = "bestaudio[ext=m4a]";

fn failed(url: &str, reason: impl Into<String>) -> SpsyncError {
    SpsyncError::Source {
        url: url.to_owned(),
        reason: reason.into(),
    }
}

/// Fetches the m4a audio stream for `url` with yt-dlp, re-validating the url before exec.
pub(crate) async fn download(url: &str) -> Result<Vec<u8>, SpsyncError> {
    validate_url(url)?;

    let run = tokio::process::Command::new("yt-dlp")
        .args([
            "--format",
            FORMAT,
            "--no-playlist",
            "--quiet",
            "--no-warnings",
            "--output",
            "-",
            "--",
        ])
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match tokio::time::timeout(TIMEOUT, run).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(failed(url, "yt-dlp is not installed in this image"));
        }
        Ok(Err(e)) => return Err(failed(url, e.to_string())),
        Err(_) => return Err(failed(url, "yt-dlp timed out")),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("yt-dlp failed")
            .to_owned();

        return Err(failed(url, reason));
    }

    if output.stdout.is_empty() {
        return Err(failed(url, "yt-dlp produced no audio"));
    }
    if output.stdout.len() > MAX_BYTES {
        return Err(failed(url, "audio is implausibly large"));
    }

    Ok(output.stdout)
}
