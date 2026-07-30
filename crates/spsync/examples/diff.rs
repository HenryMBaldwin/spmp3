use std::error::Error;

use spsync::{Client, Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let client = Client::new(Config::from_env()?)?;

    let manifest = client.manifest()?;
    println!("manifest: {} entries", manifest.len());

    let diff = client.sync_diff().await?;
    println!("to add:    {}", diff.add.len());
    println!("to remove: {}", diff.remove.len());

    for track in diff.add.iter().take(5) {
        println!("  + {} (added_at {:?})", track.uri, track.added_at);
    }

    Ok(())
}
