//! The browser has nothing to install, so every download is refused there. Same signatures as
//! `download.rs`, so the callers compile unchanged.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Result, bail};

use crate::{ReleaseChannel, SignedEnvelope, Source};

pub struct Client;

const REFUSED: &str = "qrate in the browser does not download updates or components";

pub fn client() -> Result<Client> {
    bail!(REFUSED)
}

pub fn try_mirror<T>(
    mirrored: &str,
    _original: &str,
    mut fetch: impl FnMut(&str) -> Result<T>,
) -> Result<T> {
    fetch(mirrored)
}

pub fn read_bytes(_: &Client, _: &str, _: u64) -> Result<Vec<u8>> {
    bail!(REFUSED)
}

pub(crate) fn fetch_feed(_: &Client, _: &Source, _: ReleaseChannel) -> Result<SignedEnvelope> {
    bail!(REFUSED)
}

pub fn download_verified(
    _: &str,
    _: u64,
    _: &str,
    _: &Path,
    _: impl FnMut(u64, u64),
    _: &AtomicBool,
) -> Result<()> {
    bail!(REFUSED)
}
