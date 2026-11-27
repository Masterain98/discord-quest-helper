use crate::network::HttpClient;
use reqwest::{header, Url};
use std::time::Duration;
use tauri::http::{Method, Request, Response, StatusCode};

const MAX_MEDIA_BYTES: usize = 24 * 1024 * 1024;
static CLIENT: HttpClient = HttpClient::new();

pub(crate) fn discord_asset(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    (url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("cdn.discordapp.com" | "media.discordapp.net")
        )
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

pub(crate) async fn respond(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    match download(request).await {
        Ok(response) => response,
        Err(status) => Response::builder()
            .status(status)
            .body(Vec::new())
            .expect("valid media error response"),
    }
}

async fn download(request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, StatusCode> {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return Err(StatusCode::METHOD_NOT_ALLOWED);
    }
    let query = request
        .uri()
        .query()
        .filter(|query| query.len() <= 8192)
        .ok_or(StatusCode::BAD_REQUEST)?;
    let url = url::form_urlencoded::parse(query.as_bytes())
        .find_map(|(key, value)| (key == "source").then(|| discord_asset(&value)).flatten())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let client = CLIENT
        .get(url.as_str(), |builder| {
            builder
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(25))
        })
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let head = request.method() == Method::HEAD;
    let mut upstream = client.request(request.method().clone(), url);
    if let Some(range) = request.headers().get(header::RANGE) {
        let range = range
            .to_str()
            .ok()
            .and_then(|value| value.strip_prefix("bytes="));
        let valid = range
            .and_then(|value| value.split_once('-'))
            .is_some_and(|(start, end)| {
                !(start.is_empty() && end.is_empty())
                    && [start, end]
                        .into_iter()
                        .all(|value| value.is_empty() || value.parse::<u64>().is_ok())
            });
        if !valid {
            return Err(StatusCode::RANGE_NOT_SATISFIABLE);
        }
        upstream = upstream.header(header::RANGE, request.headers()[header::RANGE].clone());
    }

    let mut upstream = upstream.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status = upstream.status();
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        let mut response = Response::builder().status(status);
        if let Some(range) = upstream.headers().get(header::CONTENT_RANGE) {
            response = response.header(header::CONTENT_RANGE, range);
        }
        return response
            .body(Vec::new())
            .map_err(|_| StatusCode::BAD_GATEWAY);
    }
    if !matches!(status, StatusCode::OK | StatusCode::PARTIAL_CONTENT) {
        return Err(if status == StatusCode::NOT_FOUND {
            status
        } else {
            StatusCode::BAD_GATEWAY
        });
    }

    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .ok_or(StatusCode::UNSUPPORTED_MEDIA_TYPE)?;
    if !matches!(
        content_type,
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "video/mp4"
            | "video/webm"
    ) {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "private, max-age=300")
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Security-Policy", "default-src 'none'");
    for name in [header::ACCEPT_RANGES, header::CONTENT_RANGE] {
        if let Some(value) = upstream.headers().get(&name) {
            response = response.header(name, value);
        }
    }

    let mut body = Vec::new();
    if head {
        if let Some(length) = upstream.headers().get(header::CONTENT_LENGTH) {
            response = response.header(header::CONTENT_LENGTH, length);
        }
    } else {
        if upstream
            .content_length()
            .is_some_and(|length| length > MAX_MEDIA_BYTES as u64)
        {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        while let Some(chunk) = upstream
            .chunk()
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?
        {
            if body.len() + chunk.len() > MAX_MEDIA_BYTES {
                return Err(StatusCode::PAYLOAD_TOO_LARGE);
            }
            body.extend_from_slice(&chunk);
        }
        response = response.header(header::CONTENT_LENGTH, body.len());
    }
    response.body(body).map_err(|_| StatusCode::BAD_GATEWAY)
}
