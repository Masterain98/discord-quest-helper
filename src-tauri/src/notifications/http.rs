use super::error::{Code, Result};
use std::sync::{Arc, OnceLock};

/// Shared and lazy: TLS/proxy setup failures affect network deliveries, never app startup.
#[derive(Clone, Default)]
pub struct Client(Arc<OnceLock<Result<reqwest::Client>>>);
impl Client {
    pub fn get(&self) -> Result<&reqwest::Client> {
        self.0.get_or_init(client).as_ref().map_err(Clone::clone)
    }
    #[cfg(test)]
    pub(super) fn unavailable() -> Self {
        let value = OnceLock::new();
        let _ = value.set(Err(Code::Network.into()));
        Self(Arc::new(value))
    }
}

pub fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        // Disable reqwest's protocol-level retries as well as application retries.
        .retry(reqwest::retry::never())
        .build()
        .map_err(|_| Code::Network.into())
}
pub async fn submit(request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    // reqwest errors may contain credential-bearing URLs. Never format them.
    request.send().await.map_err(|e| {
        if e.is_timeout() {
            Code::Timeout
        } else {
            Code::Network
        }
        .into()
    })
}
pub async fn json(response: reqwest::Response) -> Result<serde_json::Value> {
    match response.status().as_u16() {
        401 => return Err(Code::Unauthorized.into()),
        403 => return Err(Code::Forbidden.into()),
        404 => return Err(Code::NotFound.into()),
        429 => return Err(Code::RateLimited.into()),
        _ => {}
    }
    if !response.status().is_success() {
        return Err(Code::Rejected.into());
    }
    response.json().await.map_err(|e| {
        if e.is_timeout() {
            Code::Timeout
        } else {
            Code::InvalidResponse
        }
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn timeout_does_not_echo_urls_or_retry() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            let _ = socket.read(&mut buffer).await;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_millis(100))
            .build()
            .unwrap();
        let error = submit(client.get(format!("http://{addr}/dummy-credential")))
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, Code::Timeout);
        assert!(!serde_json::to_string(&error)
            .unwrap()
            .contains("dummy-credential"));
        server.abort();
    }
}
