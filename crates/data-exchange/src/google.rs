//! Creating a Google Sheet in the user's own Drive.
//!
//! This is the one export that needs the user's permission rather than a file path, so it carries
//! an OAuth flow: qrate binds a loopback port, the user consents in their browser, and Google
//! redirects the code back to that port. The scope is `drive.file`, which reaches only the files
//! qrate itself creates — consenting here does not hand qrate the rest of a Drive.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Baked in at build time. Google issues these per project and treats the "secret" of an installed
/// app as non-confidential (it ships inside every copy), so the loopback + PKCE pair below is what
/// actually protects the exchange.
const CLIENT_ID: Option<&str> = option_env!("QRATE_GOOGLE_CLIENT_ID");
const CLIENT_SECRET: Option<&str> = option_env!("QRATE_GOOGLE_CLIENT_SECRET");
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

#[derive(Debug, Error)]
pub enum GoogleError {
    #[error("This build of qrate has no Google client ID, so it can't sign in to Google")]
    NotConfigured,
    #[error("We couldn't reach Google — {0}")]
    Http(#[from] reqwest::Error),
    #[error("We couldn't listen for Google's reply — {0}")]
    Io(#[from] std::io::Error),
    #[error("Google's sign-in was cancelled or didn't come back")]
    NoCode,
    #[error("Google turned the sign-in down — {0}")]
    Denied(String),
}

fn client() -> Result<reqwest::blocking::Client, reqwest::Error> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
}

/// URL-safe random, for the PKCE verifier and the state parameter.
fn nonce() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS always has randomness");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A consent request waiting to be answered: the port is already bound, so the browser can't come
/// back before we're listening. Split in two because opening a URL is the caller's job (gpui's
/// `cx.open_url`) while waiting for the reply blocks and belongs on a background thread.
pub struct Consent {
    pub url: String,
    listener: TcpListener,
    verifier: String,
    state: String,
    redirect: String,
}

pub fn begin_consent() -> Result<Consent, GoogleError> {
    let client_id = CLIENT_ID.ok_or(GoogleError::NotConfigured)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let (verifier, state) = (nonce(), nonce());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={client_id}\
         &redirect_uri={redirect}&response_type=code&scope={SCOPE}&state={state}\
         &code_challenge={challenge}&code_challenge_method=S256&access_type=offline&prompt=consent"
    );
    Ok(Consent {
        url,
        listener,
        verifier,
        state,
        redirect,
    })
}

/// What the caller stores. `refresh` is only present the first time a user consents, so a `None`
/// means "keep the one you already had".
pub struct Token {
    pub access: String,
    pub refresh: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    error_description: Option<String>,
    error: Option<String>,
}

impl TokenResponse {
    fn into_token(self) -> Result<Token, GoogleError> {
        match self.access_token {
            Some(access) => Ok(Token {
                access,
                refresh: self.refresh_token,
            }),
            None => Err(GoogleError::Denied(
                self.error_description
                    .or(self.error)
                    .unwrap_or_else(|| "no reason given".into()),
            )),
        }
    }
}

impl Consent {
    /// Blocks until the browser hits the loopback port, then trades the code for tokens.
    pub fn wait_for_token(self) -> Result<Token, GoogleError> {
        let mut stream = self
            .listener
            .incoming()
            .next()
            .ok_or(GoogleError::NoCode)??;
        let mut request = String::new();
        BufReader::new(&stream).read_line(&mut request)?;
        // "GET /?code=…&state=… HTTP/1.1" — everything we need is in the first line.
        let query = request
            .split_whitespace()
            .nth(1)
            .and_then(|path| path.split_once('?'))
            .map(|(_, query)| query.to_string())
            .unwrap_or_default();
        let param = |wanted: &str| {
            query.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                (key == wanted).then(|| urldecode(value))
            })
        };
        let code = param("code");
        let answered = param("state").as_deref() == Some(&self.state);
        let _ = stream.write_all(
            if code.is_some() && answered {
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                 <h2>qrate is signed in</h2><p>You can close this tab.</p>"
            } else {
                "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n\
                 <h2>Sign-in didn't complete</h2><p>Close this tab and try the export again.</p>"
            }
            .as_bytes(),
        );

        // A mismatched state means this reply is not the one we asked for, so it is not a code.
        let (Some(code), true) = (code, answered) else {
            return Err(GoogleError::NoCode);
        };
        let mut form = vec![
            ("code", code),
            ("client_id", CLIENT_ID.unwrap_or_default().to_string()),
            ("grant_type", "authorization_code".into()),
            ("redirect_uri", self.redirect),
            ("code_verifier", self.verifier),
        ];
        if let Some(secret) = CLIENT_SECRET {
            form.push(("client_secret", secret.to_string()));
        }
        client()?
            .post("https://oauth2.googleapis.com/token")
            .form(&form)
            .send()?
            .json::<TokenResponse>()?
            .into_token()
    }
}

/// Trade a stored refresh token for a fresh access token, so consent is once per machine.
pub fn refresh(refresh_token: &str) -> Result<Token, GoogleError> {
    let client_id = CLIENT_ID.ok_or(GoogleError::NotConfigured)?;
    let mut form = vec![
        ("refresh_token", refresh_token.to_string()),
        ("client_id", client_id.to_string()),
        ("grant_type", "refresh_token".to_string()),
    ];
    if let Some(secret) = CLIENT_SECRET {
        form.push(("client_secret", secret.to_string()));
    }
    client()?
        .post("https://oauth2.googleapis.com/token")
        .form(&form)
        .send()?
        .json::<TokenResponse>()?
        .into_token()
}

#[derive(Deserialize)]
struct CreatedSheet {
    #[serde(rename = "spreadsheetId")]
    id: String,
    #[serde(rename = "spreadsheetUrl")]
    url: String,
}

/// Create a spreadsheet in the user's Drive and fill its first tab. Returns the URL to open.
pub fn create_sheet(
    token: &str,
    title: &str,
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<String, GoogleError> {
    let client = client()?;
    let created: CreatedSheet = client
        .post("https://sheets.googleapis.com/v4/spreadsheets")
        .bearer_auth(token)
        .json(&serde_json::json!({ "properties": { "title": title } }))
        .send()?
        .error_for_status()?
        .json()?;

    let mut values: Vec<&[String]> = vec![headers];
    values.extend(rows.iter().map(Vec::as_slice));
    // No sheet name in the range: a new spreadsheet has exactly one tab, and its title is
    // localised — "A1" alone means "the first tab", which is the one we just made.
    client
        .put(format!(
            "https://sheets.googleapis.com/v4/spreadsheets/{}/values/A1?valueInputOption=RAW",
            created.id
        ))
        .bearer_auth(token)
        .json(&serde_json::json!({ "values": values }))
        .send()?
        .error_for_status()?;
    Ok(created.url)
}

/// Percent-decoding for the one query string we read back off the loopback socket.
fn urldecode(raw: &str) -> String {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&raw[i + 1..i + 3], 16) {
                Ok(byte) => {
                    out.push(byte);
                    i += 3;
                }
                Err(_) => {
                    out.push(bytes[i]);
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::urldecode;

    #[test]
    fn decodes_the_pieces_a_redirect_actually_carries() {
        assert_eq!(urldecode("4%2F0Ab-cd_ef"), "4/0Ab-cd_ef");
        assert_eq!(urldecode("a+b"), "a b");
        // A stray `%` is data, not the start of an escape — decoding must not eat the rest.
        assert_eq!(urldecode("100%"), "100%");
        assert_eq!(urldecode("%zz"), "%zz");
    }
}
