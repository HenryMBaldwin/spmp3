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

pub(crate) const DEFAULT_OAUTH_PORT: u16 = 5588;

pub(crate) async fn interactive_login(
    client_id: String,
    port: u16,
    open_browser: bool,
) -> Result<Credentials, SpsyncError> {
    let redirect_uri = format!("http://127.0.0.1:{port}/login");

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
