use librespot_core::authentication::Credentials;
use librespot_oauth::OAuthClientBuilder;

use crate::error::SpsyncError;

const OAUTH_SCOPES: &[&str] = &[
    "streaming",
    "user-library-read",
    "user-read-email",
    "user-read-private",
    "playlist-read-private",
    "playlist-read-collaborative",
];

pub(crate) async fn interactive_login(
    client_id: String,
    redirect_host: &str,
    port: u16,
    open_browser: bool,
) -> Result<Credentials, SpsyncError> {
    let redirect_uri = format!("http://{redirect_host}:{port}/login");
    tracing::info!(redirect_uri = %redirect_uri, "starting oauth flow");

    let token = tokio::task::spawn_blocking(move || {
        let mut builder = OAuthClientBuilder::new(&client_id, &redirect_uri, OAUTH_SCOPES.to_vec());
        if open_browser {
            builder = builder.open_in_browser();
        }
        builder.build()?.get_access_token()
    })
    .await
    .map_err(|_| SpsyncError::LoginAborted)??;

    Ok(Credentials::with_access_token(token.access_token))
}
