//! The app side of Google sync: whether it is switched on, which Cloud project it signs in
//! against, and where the user's grant is kept.
//!
//! `data-exchange` owns the protocol and knows nothing about settings; this module owns the three
//! decisions that need `cx` — the opt-in gate, the credential resolution ladder, and the keychain.

use std::time::{SystemTime, UNIX_EPOCH};

use data_exchange::google::{ClientCreds, DEFAULT_CONFIG_ENDPOINT};
use gpui::App;
use settings::{AppSettings, GOOGLE_CONFIG_ENDPOINT_KEY};

/// The platform credential store entry. Windows Credential Manager, macOS Keychain, or the
/// Secret Service on Linux.
const KEYCHAIN_SERVICE: &str = "qrate";
const KEYCHAIN_ACCOUNT: &str = "google-refresh-token";

/// Where the fetched pair is remembered between launches. These are the *application's*
/// credentials, the same for every user, so `AppSettings` is the right home — unlike the refresh
/// token, which is one person's grant and goes in the keychain.
const CLIENT_ID_KEY: &str = "google_client_id";
const CLIENT_SECRET_KEY: &str = "google_client_secret";
const ETAG_KEY: &str = "google_config_etag";
const CHECKED_AT_KEY: &str = "google_config_checked_at";

/// How long a fetched pair is trusted before the next Google action re-checks it. Long enough that
/// the endpoint is off the critical path; short enough that a rotation reaches users within a week.
const CHECK_INTERVAL: u64 = 7 * 24 * 60 * 60;

/// The credentials to sign in with, and whether the endpoint should be re-checked first.
///
/// Split from the fetch because the check blocks on the network and this reads globals: the caller
/// reads here on the main thread, hands [`Stored::refreshed`] to a background thread, and writes
/// the answer back.
pub struct Stored {
    pub creds: Option<ClientCreds>,
    pub endpoint: String,
    pub etag: Option<String>,
    pub stale: bool,
}

pub fn stored(cx: &App) -> Stored {
    let values = &AppSettings::get(cx).values;
    let text = |key: &str| {
        values
            .get(key)
            .map(|v| v.text().to_string())
            .filter(|s| !s.is_empty())
    };
    let checked_at: u64 = text(CHECKED_AT_KEY)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Stored {
        creds: text(CLIENT_ID_KEY).map(|client_id| ClientCreds {
            client_id,
            client_secret: text(CLIENT_SECRET_KEY),
        }),
        endpoint: text(GOOGLE_CONFIG_ENDPOINT_KEY)
            .unwrap_or_else(|| DEFAULT_CONFIG_ENDPOINT.to_string()),
        etag: text(ETAG_KEY),
        stale: now().saturating_sub(checked_at) > CHECK_INTERVAL,
    }
}

impl Stored {
    /// The resolution ladder, run on a background thread: the persisted copy while it is fresh,
    /// otherwise a conditional re-check and then the persisted copy, and the compiled-in pair if
    /// nothing was ever persisted.
    ///
    /// Returns the credentials to use and, when the endpoint answered with a new pair, what to
    /// persist. A network failure keeps whatever is stored — working offline is the normal case
    /// for this audience, so it must never block an export.
    pub fn refreshed(self) -> (Option<ClientCreds>, Option<(ClientCreds, Option<String>)>) {
        if !self.stale && self.creds.is_some() {
            return (self.creds, None);
        }
        match data_exchange::google::fetch_config(&self.endpoint, self.etag.as_deref()) {
            Ok(Some(config)) => (
                Some(config.creds.clone()),
                Some((config.creds, config.etag)),
            ),
            // 304 — what we have is current, so only the check time moves.
            Ok(None) => (
                self.creds.clone(),
                self.creds.map(|creds| (creds, self.etag)),
            ),
            Err(err) => {
                log::warn!(
                    "could not check the Google credential endpoint {}, carrying on with what is \
                     stored: {err}",
                    self.endpoint
                );
                (self.creds.or_else(ClientCreds::compiled_in), None)
            }
        }
    }
}

/// Remember what the endpoint answered, and that it was asked just now.
pub fn remember(creds: &ClientCreds, etag: Option<String>, cx: &mut App) {
    AppSettings::set_text(CLIENT_ID_KEY, creds.client_id.clone().into(), cx);
    AppSettings::set_text(
        CLIENT_SECRET_KEY,
        creds.client_secret.clone().unwrap_or_default().into(),
        cx,
    );
    AppSettings::set_text(ETAG_KEY, etag.unwrap_or_default().into(), cx);
    AppSettings::set_text(CHECKED_AT_KEY, now().to_string().into(), cx);
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn entry() -> Option<keyring::Entry> {
    match keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
        Ok(entry) => Some(entry),
        Err(err) => {
            log::warn!(
                "this machine has no usable credential store (Credential Manager, Keychain or \
                 Secret Service), so qrate will ask for Google consent every time: {err}"
            );
            None
        }
    }
}

/// The stored grant, if the user has consented on this machine and the credential store still has
/// it. `None` means consent runs again — which is the whole fallback, since qrate never writes the
/// token anywhere else.
pub fn refresh_token() -> Option<String> {
    entry()?.get_password().ok()
}

pub fn set_refresh_token(token: &str) {
    if let Some(entry) = entry()
        && let Err(err) = entry.set_password(token)
    {
        log::warn!("could not store the Google sign-in in the credential store: {err}");
    }
}

/// Switching Google sync off has to end the grant, not just hide the buttons — otherwise "off"
/// leaves a live token on the machine.
pub fn clear_refresh_token() {
    if let Some(entry) = entry()
        && let Err(err) = entry.delete_credential()
        && !matches!(err, keyring::Error::NoEntry)
    {
        log::warn!("could not remove the stored Google sign-in: {err}");
    }
}

#[cfg(test)]
mod tests {
    use data_exchange::google::ClientCreds;

    use super::Stored;

    /// An endpoint nothing listens on: a connection refused in milliseconds, which is the failure
    /// every offline user hits.
    const DEAD: &str = "http://127.0.0.1:1/oauth/config";

    fn stored(creds: Option<ClientCreds>, stale: bool) -> Stored {
        Stored {
            creds,
            endpoint: DEAD.into(),
            etag: None,
            stale,
        }
    }

    fn creds() -> ClientCreds {
        ClientCreds {
            client_id: "stored-id".into(),
            client_secret: Some("stored-secret".into()),
        }
    }

    /// A fresh copy is the answer on its own — reaching the endpoint at all would make every
    /// export wait on the network.
    #[test]
    fn a_fresh_copy_is_used_without_asking_the_endpoint() {
        let (resolved, persist) = stored(Some(creds()), false).refreshed();
        assert_eq!(resolved, Some(creds()));
        assert!(persist.is_none());
    }

    /// Offline is the normal case for this audience, so an unreachable endpoint has to leave the
    /// stored pair in place rather than fail the sign-in.
    #[test]
    fn an_unreachable_endpoint_keeps_what_is_stored() {
        let (resolved, persist) = stored(Some(creds()), true).refreshed();
        assert_eq!(resolved, Some(creds()));
        assert!(persist.is_none());
    }

    /// Nothing stored and nothing reachable falls through to the build-time pair, which is absent
    /// in a test build — so this is the "no credentials anywhere" answer, not a panic.
    #[test]
    fn with_nothing_stored_it_falls_through_to_the_compiled_in_pair() {
        let (resolved, _) = stored(None, true).refreshed();
        assert_eq!(resolved, ClientCreds::compiled_in());
    }
}
