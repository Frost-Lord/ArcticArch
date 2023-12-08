use tokio::net::TcpStream;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use std::fs;
use std::io::Result;
use std::path::Path;
use std::collections::HashMap;
use tokio::sync::Mutex;
use lazy_static::lazy_static;
use cookie::{Cookie, CookieJar};
use std::sync::Arc;

mod api;

lazy_static! {
    static ref CACHE: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
    pub static ref SESSIONS: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
}


pub async fn handle_request(configs: Arc<Mutex<Vec<(String, bool, String, String, String)>>>, mut stream: TcpStream) -> Result<()> {
    let mut buffer = [0; 1024];
    let _ = stream.read(&mut buffer).await?;

    let request_line = std::str::from_utf8(&buffer).unwrap_or("");
    let request_path = request_line
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");

    if request_path.starts_with("/api/") {
        let response = api::handle_api_request(configs, request_path, &buffer).await?;
        stream.write_all(response.as_bytes()).await?;
    } else {
        serve_html_file(&mut stream, request_path, &buffer).await?;
    }

    stream.flush().await
}

async fn serve_html_file(stream: &mut TcpStream, request_path: &str, buffer: &[u8]) -> Result<()> {
    let no_session_required = request_path == "/" || request_path.starts_with("/styles/") || request_path == "/login";

    if !no_session_required {
        let buffer_string = String::from_utf8_lossy(buffer).to_string();
        let cookies_str = buffer_string
            .lines()
            .find(|line| line.starts_with("Cookie:"))
            .and_then(|line| line.split_whitespace().nth(1));

        let mut cookie_jar = CookieJar::new();
        if let Some(cookies_str) = cookies_str {
            cookies_str.split(';').for_each(|cookie_str| {
                if let Ok(cookie) = Cookie::parse(cookie_str.trim().to_owned()) {
                    cookie_jar.add(cookie);
                }
            });
        }

        if let Some(cookie) = cookie_jar.get("session") {
            let session_id = cookie.value();
            let sessions = SESSIONS.lock().await;
            if sessions.contains_key(session_id) {
                let file_path = format!("./default{}.html", request_path);
                if let Some(contents) = cache_get(&file_path).await {
                    stream.write_all(contents.as_bytes()).await?;
                } else {
                    let response = cache_get("./default/404.html").await.unwrap_or_else(|| String::from("HTTP/1.1 404 NOT FOUND\r\n\r\n"));
                    stream.write_all(response.as_bytes()).await?;
                }
            } else {
                stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /\r\n\r\n").await?;
            }
        } else {
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /\r\n\r\n").await?;
        }
    } else {
        let file_path = if request_path == "/" {
            "./default/login.html".to_string()
        } else {
            format!("./default{}.html", request_path)
        };
    
        let response = if let Some(contents) = cache_get(&file_path).await {
            contents
        } else if request_path.starts_with("/styles/") {
            serve_css_file(&buffer).await?
        } else {
            cache_get("./default/404.html").await.unwrap_or_else(|| String::from("HTTP/1.1 404 NOT FOUND\r\n\r\n"))
        };
        stream.write_all(response.as_bytes()).await?;
    }

    stream.flush().await?;
    Ok(())
}


async fn cache_get(path: &str) -> Option<String> {
    let mut cache = CACHE.lock().await;
    if cache.contains_key(path) {
        Some(cache.get(path).unwrap().clone())
    } else {
        if Path::new(path).exists() {
            let contents = fs::read_to_string(path).ok()?;
            cache.insert(path.to_string(), format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", contents.len(), contents));
            Some(cache.get(path).unwrap().clone())
        } else {
            None
        }
    }
}

async fn serve_css_file(buffer: &[u8]) -> Result<String> {
    let request_line = std::str::from_utf8(buffer).unwrap_or("");
    let css_file_path = request_line
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .map(|path| format!("./default{}", path))
        .unwrap_or_default();

    if Path::new(&css_file_path).exists() {
        let contents = fs::read_to_string(css_file_path)?;
        Ok(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nContent-Length: {}\r\n\r\n{}",
            contents.len(),
            contents
        ))
    } else {
        Ok(String::from("HTTP/1.1 404 NOT FOUND\r\n\r\n"))
    }
}
