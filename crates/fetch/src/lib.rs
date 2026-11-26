//! One HTTP round trip over gpui's `HttpClient`: reqwest on the desktop, `fetch` in the browser.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::AsyncReadExt;
use http_client::RedirectPolicy;
use serde::de::DeserializeOwned;

pub use http_client::http::{HeaderMap, header};
pub use http_client::{
    AsyncBody, HttpClient, HttpRequestExt, Method, Request, Response, StatusCode, Url,
    http::request::Builder,
};

/// Every request follows redirects and gives up after 30 seconds, so a stalled server returns an
/// error instead of holding a background task forever.
pub fn request(method: Method, url: &str) -> Builder {
    Request::builder()
        .method(method)
        .uri(url)
        .follow_redirects(RedirectPolicy::FollowAll)
        .timeout(Duration::from_secs(30))
}

/// A response read to the end.
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    /// Turns a non-2xx status into an error, as `reqwest`'s `error_for_status` did.
    pub fn ok(self) -> Result<Self> {
        if !self.status.is_success() {
            bail!("the server answered {}", self.status);
        }
        Ok(self)
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).context("the server's reply was not the JSON expected")
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: header::HeaderName) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

pub async fn send(client: &dyn HttpClient, request: Request<AsyncBody>) -> Result<Reply> {
    let mut response = client.send(request).await?;
    let mut body = Vec::new();
    response
        .body_mut()
        .read_to_end(&mut body)
        .await
        .context("the connection dropped while reading the reply")?;
    Ok(Reply {
        status: response.status(),
        headers: response.headers().clone(),
        body,
    })
}

pub async fn get(client: &dyn HttpClient, url: &str) -> Result<Reply> {
    send(client, request(Method::GET, url).body(AsyncBody::empty())?).await
}

pub fn json(builder: Builder, value: &impl serde::Serialize) -> Result<Request<AsyncBody>> {
    let body = serde_json::to_vec(value)?;
    Ok(builder
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.into())?)
}

pub fn form(builder: Builder, fields: &impl serde::Serialize) -> Result<Request<AsyncBody>> {
    let body = serde_urlencoded::to_string(fields)?;
    Ok(builder
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body.into())?)
}
