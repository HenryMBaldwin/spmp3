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
    let session = client.session().await?;

    let uri = format!("spotify:user:{}:collection", session.username());
    println!("requesting context: {uri}");

    let context = session.spclient().get_context(&uri).await?;

    println!("context.uri   = {:?}", context.uri);
    println!("context.url   = {:?}", context.url);
    println!("context.pages = {}", context.pages.len());
    println!("metadata      = {:?}", context.metadata);

    for (i, page) in context.pages.iter().enumerate() {
        println!(
            "\npage {i}: tracks={} page_url={:?} next_page_url={:?}",
            page.tracks.len(),
            page.page_url,
            page.next_page_url
        );
        println!("  metadata = {:?}", page.metadata);
        for track in page.tracks.iter().take(3) {
            println!("  track uri={:?} metadata={:?}", track.uri, track.metadata);
        }
    }

    Ok(())
}
