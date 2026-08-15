//! Creating a Google Sheet in the user's own Drive.
//!
//! This is the one export that needs the user's permission rather than a file path, so it carries
//! an OAuth flow: qrate binds a loopback port, the user consents in their browser, and Google
//! redirects the code back to that port. The scope is `drive.file`, which reaches only the files
//! qrate itself creates — consenting here does not hand qrate the rest of a Drive.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Baked in at build time. Google issues these per project and treats the "secret" of an installed
/// app as non-confidential (it ships inside every copy), so the loopback + PKCE pair below is what
/// actually protects the exchange.
const CLIENT_ID: Option<&str> = option_env!("QRATE_GOOGLE_CLIENT_ID");
const CLIENT_SECRET: Option<&str> = option_env!("QRATE_GOOGLE_CLIENT_SECRET");
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

/// Sent as a bearer to the credential endpoint. It ships inside the binary, so it stops casual
/// scraping and drive-by indexing — it is not access control, and what it guards is not
/// confidential anyway (see the module docs on the client secret).
const CONFIG_TOKEN: Option<&str> = option_env!("QRATE_GOOGLE_CONFIG_TOKEN");

/// Where [`fetch_config`] looks unless the user points it somewhere else. Anyone can run the same
/// contract on their own infrastructure — see `docs/site-oauth-handoff.md`.
pub const DEFAULT_CONFIG_ENDPOINT: &str = "https://qrate.dvnl.work/oauth/config";

/// Where the Google Picker page lives. `drive.file` reaches only files the app created or the user
/// picked *through the Picker*, so this page is the sole route to an already-owned spreadsheet.
pub const DEFAULT_PICKER_PAGE: &str = "https://qrate.dvnl.work/picker";

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
    #[error(
        "qrate hasn't been given access to that spreadsheet. Google only grants access to sheets \
         you pick through its own chooser, so open File ▸ Export ▸ Sync to Google Sheet and choose \
         it there."
    )]
    NoAccess,
}

/// Which Google Cloud project qrate signs in against. Passed in rather than read from the consts
/// below, because the live pair can come from the credential endpoint instead — see
/// [`fetch_config`] and `crates/app/src/google.rs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCreds {
    pub client_id: String,
    /// Absent for a client created without one. Google does not require it for an installed app.
    #[serde(default)]
    pub client_secret: Option<String>,
}

impl ClientCreds {
    /// The build-time pair — the last rung of the resolution ladder, used when nothing has ever
    /// been fetched or persisted.
    pub fn compiled_in() -> Option<Self> {
        CLIENT_ID.map(|client_id| Self {
            client_id: client_id.to_string(),
            client_secret: CLIENT_SECRET.map(str::to_string),
        })
    }

    /// The pieces every token request carries.
    fn form(&self) -> Vec<(&'static str, String)> {
        let mut form = vec![("client_id", self.client_id.clone())];
        if let Some(secret) = &self.client_secret {
            form.push(("client_secret", secret.clone()));
        }
        form
    }
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
    creds: ClientCreds,
}

pub fn begin_consent(creds: &ClientCreds) -> Result<Consent, GoogleError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let (verifier, state) = (nonce(), nonce());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let mut url = reqwest::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
        .expect("the constant Google authorization URL is valid");
    url.query_pairs_mut()
        .append_pair("client_id", &creds.client_id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    let url = url.to_string();
    Ok(Consent {
        url,
        listener,
        verifier,
        state,
        redirect,
        creds: creds.clone(),
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

/// Accept the one loopback request we are waiting for, answer the browser, and hand back its
/// query parameters. Shared by the consent redirect and the Picker's return trip, which differ
/// only in which parameter they came for.
///
/// A `state` that doesn't match means this request is not the reply we asked for, so it yields
/// nothing rather than being trusted.
fn wait_for_query(
    listener: &TcpListener,
    state: &str,
    done: &str,
) -> Result<HashMap<String, String>, GoogleError> {
    let mut stream = listener.incoming().next().ok_or(GoogleError::NoCode)??;
    let mut request = String::new();
    BufReader::new(&stream).read_line(&mut request)?;
    // "GET /?code=…&state=… HTTP/1.1" — everything we need is in the first line.
    let params: HashMap<String, String> = request
        .split_whitespace()
        .nth(1)
        .and_then(|path| path.split_once('?'))
        .map(|(_, query)| query.to_string())
        .unwrap_or_default()
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((key.to_string(), urldecode(value)))
        })
        .collect();

    let answered = params.get("state").map(String::as_str) == Some(state);
    let _ = stream.write_all(
        if answered {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                 <h2>{done}</h2><p>You can close this tab.</p>"
            )
        } else {
            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n\
             <h2>That didn't complete</h2><p>Close this tab and try again in qrate.</p>"
                .to_string()
        }
        .as_bytes(),
    );
    answered.then_some(params).ok_or(GoogleError::NoCode)
}

impl Consent {
    /// Blocks until the browser hits the loopback port, then trades the code for tokens.
    pub fn wait_for_token(self) -> Result<Token, GoogleError> {
        let params = wait_for_query(&self.listener, &self.state, "qrate is signed in")?;
        let code = params.get("code").ok_or(GoogleError::NoCode)?;
        let mut form = self.creds.form();
        form.extend([
            ("code", code.clone()),
            ("grant_type", "authorization_code".into()),
            ("redirect_uri", self.redirect),
            ("code_verifier", self.verifier),
        ]);
        client()?
            .post("https://oauth2.googleapis.com/token")
            .form(&form)
            .send()?
            .json::<TokenResponse>()?
            .into_token()
    }
}

/// Trade a stored refresh token for a fresh access token, so consent is once per machine.
pub fn refresh(creds: &ClientCreds, refresh_token: &str) -> Result<Token, GoogleError> {
    let mut form = creds.form();
    form.extend([
        ("refresh_token", refresh_token.to_string()),
        ("grant_type", "refresh_token".to_string()),
    ]);
    client()?
        .post("https://oauth2.googleapis.com/token")
        .form(&form)
        .send()?
        .json::<TokenResponse>()?
        .into_token()
}

/// A pending Picker choice. Same split as [`Consent`] and for the same reason: opening the URL is
/// the caller's job, waiting for the answer blocks.
pub struct Picker {
    pub url: String,
    listener: TcpListener,
    state: String,
}

/// Open the hosted Picker page, handing it the access token in the URL **fragment** — browsers
/// never send a fragment to the server, so the token reaches only that page's script.
pub fn begin_picker(page: &str, access_token: &str) -> Result<Picker, GoogleError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let state = nonce();
    let url = format!(
        "{page}#token={}&state={}&port={}",
        urlencode(access_token),
        urlencode(&state),
        listener.local_addr()?.port()
    );
    Ok(Picker {
        url,
        listener,
        state,
    })
}

impl Picker {
    /// Blocks until the page redirects back with the chosen spreadsheet's id.
    pub fn wait_for_file_id(self) -> Result<String, GoogleError> {
        wait_for_query(&self.listener, &self.state, "Spreadsheet chosen")?
            .remove("fileId")
            .ok_or(GoogleError::NoCode)
    }
}

/// What the credential endpoint serves. See `docs/site-oauth-handoff.md` for the contract.
#[derive(Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub creds: ClientCreds,
    #[serde(skip)]
    pub etag: Option<String>,
}

/// Ask the credential endpoint whether the stored pair is stale. `Ok(None)` is a `304` — what the
/// caller already has is current, which is the answer on almost every check.
pub fn fetch_config(endpoint: &str, etag: Option<&str>) -> Result<Option<Config>, GoogleError> {
    let mut request = client()?
        .get(endpoint)
        .bearer_auth(CONFIG_TOKEN.unwrap_or_default());
    if let Some(etag) = etag {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let response = request.send()?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    let response = response.error_for_status()?;
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut config: Config = response.json()?;
    config.etag = etag;
    Ok(Some(config))
}

#[derive(Deserialize)]
struct CreatedSheet {
    #[serde(rename = "spreadsheetId")]
    id: String,
    #[serde(rename = "spreadsheetUrl")]
    url: String,
}

/// Create an empty spreadsheet in the user's Drive. Returns its id and the URL to open — the id is
/// what a project stores to stay linked to it, so [`write_values`] can refill it later.
pub fn create_sheet(token: &str, title: &str) -> Result<(String, String), GoogleError> {
    let created: CreatedSheet = client()?
        .post("https://sheets.googleapis.com/v4/spreadsheets")
        .bearer_auth(token)
        .json(&serde_json::json!({ "properties": { "title": title } }))
        .send()?
        .error_for_status()?
        .json()?;
    Ok((created.id, created.url))
}

/// Fill a spreadsheet's first tab, replacing what is there. Works for one qrate just created and
/// for one the user chose through the Picker — those are the only two it can reach.
pub fn write_values(
    token: &str,
    spreadsheet_id: &str,
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<(), GoogleError> {
    let mut values: Vec<&[String]> = vec![headers];
    values.extend(rows.iter().map(Vec::as_slice));
    // No sheet name in the range: "A1" alone means the first tab, whose title is localised.
    let response = client()?
        .put(format!(
            "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}/values/A1?valueInputOption=RAW"
        ))
        .bearer_auth(token)
        .json(&serde_json::json!({ "values": values }))
        .send()?;
    // `drive.file` answers 404, not 403, for a file the token was never granted — so the plain
    // status is indistinguishable from a deleted sheet and has to be named for the user.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(GoogleError::NoAccess);
    }
    response.error_for_status()?;
    Ok(())
}

/// The link a stored spreadsheet id points at.
pub fn sheet_url(spreadsheet_id: &str) -> String {
    format!("https://docs.google.com/spreadsheets/d/{spreadsheet_id}")
}

/// Percent-encoding for the fragment [`begin_picker`] builds. Deliberately encodes everything
/// outside RFC 3986's unreserved set, so a `+` in a token can't come back out of the page's
/// `URLSearchParams` as a space.
fn urlencode(raw: &str) -> String {
    raw.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
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
    use super::{ClientCreds, urldecode, urlencode};

    #[test]
    fn decodes_the_pieces_a_redirect_actually_carries() {
        assert_eq!(urldecode("4%2F0Ab-cd_ef"), "4/0Ab-cd_ef");
        assert_eq!(urldecode("a+b"), "a b");
        // A stray `%` is data, not the start of an escape — decoding must not eat the rest.
        assert_eq!(urldecode("100%"), "100%");
        assert_eq!(urldecode("%zz"), "%zz");
    }

    /// The fragment carries an access token verbatim, so a round trip has to be lossless — and
    /// `+` in particular must not survive as a literal for the page to read back as a space.
    #[test]
    fn encodes_a_token_so_it_survives_the_fragment() {
        let token = "ya29.a0+Ae/4-_x.y~z";
        assert_eq!(urldecode(&urlencode(token)), token);
        assert!(!urlencode(token).contains('+'));
    }

    /// A client created without a secret must not send an empty one — Google reads that as a
    /// mismatch rather than as absence.
    #[test]
    fn a_secretless_client_sends_only_its_id() {
        let creds = ClientCreds {
            client_id: "id".into(),
            client_secret: None,
        };
        assert_eq!(creds.form(), vec![("client_id", "id".to_string())]);
    }
}
