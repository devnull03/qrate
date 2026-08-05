//! The dictionary catalogue: what languages exist, which are already on disk, and how to fetch
//! one that is not.
//!
//! Dictionaries come from `wooorm/dictionaries`, which republishes the Hunspell dictionaries every
//! desktop spell checker uses under one uniform layout — `dictionaries/<code>/index.{aff,dic}` —
//! so a download needs a code and nothing else. LibreOffice's own repository has no such rule and
//! would need a hand-kept path per language.
//!
//! The table is static rather than fetched. An index request would be one more thing to fail
//! offline, and the licences below have to be shown *before* a download rather than after — some
//! of these are GPL, which is a fact a user is entitled to see while choosing.

/// Where a dictionary's two files live, by catalogue code.
const RAW: &str = "https://raw.githubusercontent.com/wooorm/dictionaries/main/dictionaries";

/// Code, name for a human, and the licence the word list carries.
pub type Entry = (&'static str, &'static str, &'static str);

/// Every dictionary `wooorm/dictionaries` publishes, in the order it lists them.
pub const CATALOGUE: &[Entry] = &[
    ("bg", "Bulgarian", "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)"),
    ("br", "Breton", "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)"),
    ("ca", "Catalan", "(GPL-2.0 OR LGPL-2.1)"),
    ("ca-valencia", "Catalan (Valencia)", "(GPL-2.0 OR LGPL-2.1)"),
    ("cs", "Czech", "GPL-2.0"),
    ("cy", "Welsh", "LGPL-3.0"),
    ("da", "Danish", "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)"),
    ("de", "German", "(GPL-2.0 OR GPL-3.0)"),
    ("de-AT", "German (Austria)", "(GPL-2.0 OR GPL-3.0)"),
    ("de-CH", "German (Switzerland)", "(GPL-2.0 OR GPL-3.0)"),
    ("el", "Greek", "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)"),
    ("el-polyton", "Greek (Polyton)", "GPL-3.0"),
    ("en", "English (United States)", "(MIT AND BSD)"),
    ("en-AU", "English (Australia)", "(MIT AND BSD)"),
    ("en-CA", "English (Canada)", "(MIT AND BSD)"),
    ("en-GB", "English (United Kingdom)", "(MIT AND BSD)"),
    ("en-ZA", "English (South Africa)", "LGPL-2.1"),
    ("eo", "Esperanto", "GPL-2.0"),
    ("es", "Spanish", "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)"),
    (
        "es-AR",
        "Spanish (Argentina)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-BO",
        "Spanish (Bolivia)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-CL",
        "Spanish (Chile)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-CO",
        "Spanish (Colombia)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-CR",
        "Spanish (Costa Rica)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-CU",
        "Spanish (Cuba)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-DO",
        "Spanish (Dominican Republic)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-EC",
        "Spanish (Ecuador)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-GT",
        "Spanish (Guatemala)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-HN",
        "Spanish (Honduras)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-MX",
        "Spanish (Mexico)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-NI",
        "Spanish (Nicaragua)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-PA",
        "Spanish (Panama)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-PE",
        "Spanish (Peru)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-PH",
        "Spanish (Philippines)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-PR",
        "Spanish (Puerto Rico)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-PY",
        "Spanish (Paraguay)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-SV",
        "Spanish (El Salvador)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-US",
        "Spanish (United States of America)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-UY",
        "Spanish (Uruguay)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    (
        "es-VE",
        "Spanish (Venezuela)",
        "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)",
    ),
    ("et", "Estonian", " LGPL-2.1 "),
    ("eu", "Basque", " GPL-2.0 "),
    ("fa", "Persian", "Apache-2.0"),
    ("fo", "Faroese", "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)"),
    ("fr", "French", "MPL-2.0"),
    ("fur", "Friulian", "GPL-2.0"),
    ("fy", "Western Frisian", "GPL-3.0"),
    ("ga", "Irish", "GPL-2.0"),
    ("gd", "Scottish Gaelic", "GPL-3.0"),
    ("gl", "Galician", "GPL-3.0"),
    ("he", "Hebrew", "AGPL-3.0"),
    ("hr", "Croatian", "(LGPL-2.1 OR SISSL)"),
    ("hu", "Hungarian", "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)"),
    ("hy", "Armenian", "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)"),
    (
        "hyw",
        "Western Armenian",
        "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)",
    ),
    ("ia", "Interlingua", "GPL-3.0"),
    ("ie", "Interlingue", "Apache-2.0"),
    ("is", "Icelandic", "CC-BY-SA-3.0"),
    ("it", "Italian", "GPL-3.0"),
    ("ka", "Georgian", "MIT"),
    ("ko", "Korean", "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)"),
    ("la", "Latin", "GPL-2.0"),
    ("lb", "Luxembourgish", "EUPL-1.1"),
    ("lt", "Lithuanian", "BSD-3-Clause"),
    ("ltg", "Latgalian", "LGPL-2.1"),
    ("lv", "Latvian", "LGPL-2.1"),
    ("mk", "Macedonian", "GPL-3.0"),
    ("mn", "Mongolian", "LPPL-1.3c"),
    ("nb", "Norwegian Bokmål", "GPL-2.0"),
    ("nds", "Low German", "GPL-3.0"),
    ("ne", "Nepali", "LGPL-2.1"),
    ("nl", "Dutch", "(BSD-3-Clause OR CC-BY-3.0)"),
    ("nn", "Norwegian Nynorsk", "GPL-2.0"),
    ("oc", "Occitan", "GPL-2.0"),
    ("pl", "Polish", "(GPL-3.0 OR LGPL-3.0 OR MPL-2.0)"),
    ("pt", "Portuguese", "(LGPL-3.0 OR MPL-2.0)"),
    (
        "pt-PT",
        "Portuguese (Portugal)",
        "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)",
    ),
    ("ro", "Romanian", "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)"),
    ("ru", "Russian", "BSD-3-Clause"),
    ("rw", "Kinyarwanda", "GPL-3.0"),
    ("sk", "Slovak", "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)"),
    ("sl", "Slovenian", "(GPL-3.0 OR LGPL-2.1)"),
    (
        "sr",
        "Serbian",
        "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1 OR CC-BY-SA-3.0)",
    ),
    (
        "sr-Latn",
        "Serbian (Latin script)",
        "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1 OR CC-BY-SA-3.0)",
    ),
    ("sv", "Swedish", "LGPL-3.0"),
    ("sv-FI", "Swedish (Finland)", "LGPL-3.0"),
    ("tk", "Turkmen", "Apache-2.0"),
    ("tlh", "Klingon", "Apache-2.0"),
    ("tlh-Latn", "Klingon (Latin script)", "Apache-2.0"),
    ("tr", "Turkish", "MIT"),
    ("uk", "Ukrainian", "GPL-3.0"),
    ("vi", "Vietnamese", "GPL-2.0"),
];

/// The two languages compiled into the binary. These never need a download and cannot be removed,
/// which is what keeps spell checking working on a machine that has never been online.
pub const BUILT_IN: [&str; 2] = ["en-CA", "en"];

/// How a language shows up in the settings list, the same three states a phone's language screen
/// has: already here, already here and unremovable, or a download away.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    BuiltIn,
    Installed,
    Available,
}

/// The catalogue with each entry's current state resolved against the data dir.
pub fn listing() -> Vec<(Entry, State)> {
    CATALOGUE
        .iter()
        .map(|entry| {
            let state = if BUILT_IN.contains(&entry.0) {
                State::BuiltIn
            } else if is_installed(entry.0) {
                State::Installed
            } else {
                State::Available
            };
            (*entry, state)
        })
        .collect()
}

/// Where a downloaded dictionary's files go. Same folder [`crate::source`] reads from, so an
/// install is complete the moment both files land.
pub fn paths(code: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let dir = settings::data_dir()?.join("dictionaries");
    Some((
        dir.join(format!("{code}.aff")),
        dir.join(format!("{code}.dic")),
    ))
}

pub fn is_installed(code: &str) -> bool {
    paths(code).is_some_and(|(aff, dic)| aff.is_file() && dic.is_file())
}

/// A human name for a code, for log lines and the settings label.
pub fn name_of(code: &str) -> &str {
    CATALOGUE
        .iter()
        .find(|(c, _, _)| *c == code)
        .map_or(code, |(_, name, _)| name)
}

/// Fetch a dictionary's two files and write them where [`crate::source`] will find them.
///
/// Blocking on purpose — the caller runs it on the background executor, the same shape
/// `cloud_sync::fetch_sheet` uses. Both files are downloaded before either is written, so a
/// failure half way cannot leave an install that looks complete and parses to nothing.
pub fn download(code: &str) -> Result<(), String> {
    let (aff_path, dic_path) = paths(code).ok_or("qrate has nowhere to store dictionaries")?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let get = |file: &str| -> Result<Vec<u8>, String> {
        let url = format!("{RAW}/{code}/{file}");
        let resp = client.get(&url).send().map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("{} returned {}", url, resp.status()));
        }
        resp.bytes().map(|b| b.to_vec()).map_err(|e| e.to_string())
    };
    let (aff, dic) = (get("index.aff")?, get("index.dic")?);

    if let Some(parent) = aff_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&aff_path, aff).map_err(|e| e.to_string())?;
    std::fs::write(&dic_path, dic).map_err(|e| e.to_string())?;
    Ok(())
}

/// Delete a downloaded dictionary. Built-in languages are compiled in and cannot be removed.
pub fn remove(code: &str) -> Result<(), String> {
    if BUILT_IN.contains(&code) {
        return Err(format!("{} is built in", name_of(code)));
    }
    let (aff, dic) = paths(code).ok_or("qrate has nowhere to store dictionaries")?;
    for path in [aff, dic] {
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::catalogue::{BUILT_IN, CATALOGUE, State, listing, name_of};

    /// Both built-in codes must exist in the catalogue, or the settings list shows a language
    /// nobody can select and the shipped dictionary has no name.
    #[test]
    fn the_built_in_languages_are_in_the_catalogue() {
        for code in BUILT_IN {
            assert!(
                CATALOGUE.iter().any(|(c, _, _)| *c == code),
                "{code} ships but is not listed"
            );
        }
        assert_eq!(name_of("en-CA"), "English (Canada)");
        assert_eq!(name_of("qq"), "qq", "an unknown code is its own label");
    }

    #[test]
    fn codes_are_unique_and_every_entry_is_filled_in() {
        let mut codes: Vec<&str> = CATALOGUE.iter().map(|(c, _, _)| *c).collect();
        let total = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(
            codes.len(),
            total,
            "a duplicate code would shadow a language"
        );
        assert!(
            CATALOGUE
                .iter()
                .all(|(c, n, l)| !c.is_empty() && !n.is_empty() && !l.is_empty()),
            "every entry needs a code, a name, and a licence to show before downloading"
        );
    }

    #[test]
    fn the_listing_marks_the_built_ins() {
        let listing = listing();
        assert_eq!(listing.len(), CATALOGUE.len());
        for (entry, state) in &listing {
            if BUILT_IN.contains(&entry.0) {
                assert_eq!(*state, State::BuiltIn, "{} ships in the binary", entry.0);
            } else {
                assert_ne!(*state, State::BuiltIn);
            }
        }
    }
}

#[cfg(test)]
mod network {
    /// Hits the real repository, so it is `#[ignore]`d and run by hand. Its job is to catch the
    /// upstream layout changing under us — a 404 here is the whole feature silently broken.
    #[test]
    #[ignore]
    fn every_catalogued_dictionary_still_resolves() {
        let client = reqwest::blocking::Client::new();
        let mut broken = Vec::new();
        for (code, _, _) in super::CATALOGUE {
            let url = format!("{}/{code}/index.aff", super::RAW);
            match client.head(&url).send() {
                Ok(r) if r.status().is_success() => {}
                Ok(r) => broken.push(format!("{code}: {}", r.status())),
                Err(e) => broken.push(format!("{code}: {e}")),
            }
        }
        assert!(broken.is_empty(), "unreachable dictionaries: {broken:?}");
    }
}

#[cfg(test)]
mod round_trip {
    use crate::SpellCheck;
    use crate::catalogue::{download, is_installed, remove};

    /// The whole feature, end to end, against the live repository: fetch a language that is not
    /// built in, load it, check a word only it knows, then put the data dir back. `#[ignore]`d
    /// because it needs the network and writes to the real data dir.
    #[test]
    #[ignore]
    fn a_downloaded_dictionary_installs_and_checks() {
        assert!(!is_installed("fr"), "start from a clean data dir");
        download("fr").expect("French downloads");
        assert!(is_installed("fr"), "both files landed");

        let spell = SpellCheck::load("fr", false).expect("the downloaded dictionary parses");
        let dictionary = spell.dictionary.read().unwrap();
        assert!(dictionary.check("bonjour"), "French words pass");
        assert!(!dictionary.check("bonjoure"), "and French typos do not");
        drop(dictionary);

        remove("fr").expect("removable");
        assert!(!is_installed("fr"));
        assert!(remove("en-CA").is_err(), "a built-in cannot be removed");
    }
}
