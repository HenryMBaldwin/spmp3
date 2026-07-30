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

    if client.is_authenticated() {
        println!(
            "found cached credentials in {}",
            client.config().cache_dir.display()
        );
    } else {
        println!("no cached credentials, starting interactive login");
        client.login(false).await?;
    }

    println!("authenticated as {}", client.whoami().await?);

    Ok(())
}
