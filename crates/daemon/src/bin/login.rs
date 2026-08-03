use std::{
    error::Error,
    io::{BufRead, Write},
    net::TcpStream,
    time::Duration,
};

use spsync::{Client, Config};

const DELIVERY_ATTEMPTS: u32 = 100;
const DELIVERY_RETRY_DELAY: Duration = Duration::from_millis(100);

fn request_target(pasted: &str) -> String {
    let after_scheme = pasted
        .split_once("://")
        .map_or(pasted, |(_, rest)| rest)
        .trim();

    after_scheme
        .find('/')
        .map_or_else(|| "/".to_owned(), |i| after_scheme[i..].to_owned())
}

fn deliver(port: u16, target: &str) -> Result<(), Box<dyn Error>> {
    let mut stream = connect_when_ready(port)?;
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )?;
    stream.flush()?;

    Ok(())
}

fn connect_when_ready(port: u16) -> std::io::Result<TcpStream> {
    let mut last = None;

    for _ in 0..DELIVERY_ATTEMPTS {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(DELIVERY_RETRY_DELAY);
            }
        }
    }

    Err(last.unwrap_or_else(|| std::io::Error::other("could not reach the local oauth listener")))
}

fn read_pasted_url() -> std::io::Result<String> {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;

    if line.trim().is_empty() {
        return Err(std::io::Error::other("no url provided"));
    }

    Ok(line)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::from_env()?;
    let port = config.oauth_port;
    let client = Client::new(config)?;

    if client.is_authenticated() {
        println!("already authenticated as {}", client.whoami().await?);
        return Ok(());
    }

    println!("1. open the authorization url printed below in any browser");
    println!("2. approve access");
    println!(
        "3. your browser will fail to load http://127.0.0.1:{port}/login... — that is expected"
    );
    println!("4. copy that whole address from the address bar and paste it here\n");

    let waiting = tokio::spawn({
        let client = client.clone();
        async move { client.login(false).await }
    });

    let pasted = tokio::task::spawn_blocking(read_pasted_url).await??;
    deliver(port, &request_target(&pasted))?;

    let username = waiting.await??;
    println!("\nauthorized as {username}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::request_target;

    #[test]
    fn extracts_path_and_query() {
        assert_eq!(
            request_target("http://127.0.0.1:5588/login?code=abc&state=xyz"),
            "/login?code=abc&state=xyz"
        );
    }

    #[test]
    fn tolerates_surrounding_whitespace() {
        assert_eq!(
            request_target("  http://127.0.0.1:5588/login?code=abc\n"),
            "/login?code=abc"
        );
    }

    #[test]
    fn falls_back_to_root_without_a_path() {
        assert_eq!(request_target("http://127.0.0.1:5588"), "/");
    }
}
