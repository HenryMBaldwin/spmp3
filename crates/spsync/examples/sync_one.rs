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

    let diff = client.sync_diff().await?;
    let Some(track) = diff.add.first() else {
        println!("nothing to sync");
        return Ok(());
    };

    let report = client.sync_tracks(std::slice::from_ref(track)).await?;

    println!("added   {}", report.added);
    println!("failed  {}", report.failed.len());
    for failure in &report.failed {
        println!("  {} -> {}", failure.uri, failure.error);
    }

    Ok(())
}
