use std::{error::Error, fs};

use spsync::{Client, Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let client = Client::new(Config::from_env()?)?;

    let liked = client.list_liked().await?;
    let Some(track) = liked.first() else {
        println!("no liked songs");
        return Ok(());
    };

    println!("downloading {}", track.uri);
    let audio = client.download(track).await?;

    println!("title    {}", audio.meta.title);
    println!("album    {}", audio.meta.album);
    println!("artists  {:?}", audio.meta.artists);
    println!("track no {:?}", audio.meta.number);
    println!("duration {} ms", audio.meta.duration_ms);
    println!("format   {:?}", audio.format);
    println!("bytes    {}", audio.ogg.len());

    let out = client.config().library_dir.join("sample.ogg");
    fs::write(&out, &audio.ogg)?;
    println!("wrote    {}", out.display());

    Ok(())
}
