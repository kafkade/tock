//! Localization (i18n) framework for the tock CLI.
//!
//! Built on [Fluent](https://projectfluent.org/) via the `i18n-embed` stack.
//! Message catalogs live in `i18n/<lang>/*.ftl`; `en-US` is the always-complete
//! fallback locale that ships with every build and is embedded at compile time.
//!
//! # Usage
//!
//! Call [`init`] once at startup to negotiate the active locale, then use the
//! [`tr!`](crate::tr) macro at call sites:
//!
//! ```ignore
//! tock_cli::i18n::init(None);
//! println!("{}", tr!("task-added", sid = 7));
//! ```
//!
//! Locale resolution order (first match wins):
//! 1. explicit override passed to [`init`] (the `--lang` flag);
//! 2. the `TOCK_LANG` environment variable;
//! 3. the operating-system locale (`LANG` / `LC_*`);
//! 4. the `en-US` fallback.

use std::sync::OnceLock;

use i18n_embed::{
    DesktopLanguageRequester, LanguageLoader, LanguageRequester,
    fluent::{FluentLanguageLoader, fluent_language_loader},
    unic_langid::LanguageIdentifier,
};
use rust_embed::RustEmbed;

/// Compile-time-embedded localization assets (`i18n/<lang>/*.ftl`).
#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

/// Process-wide language loader, initialized by [`init`] or lazily by [`loader`].
static LOADER: OnceLock<FluentLanguageLoader> = OnceLock::new();

/// Environment variable that overrides the detected locale.
const LANG_ENV: &str = "TOCK_LANG";

/// Initialize the localization loader, negotiating the active locale.
///
/// `explicit` is the value of the `--lang` flag, if provided. This should be
/// called once, early in `main`, before any [`tr!`](crate::tr) call. Calling it
/// after the loader has already been initialized is a no-op.
pub fn init(explicit: Option<&str>) {
    let _ = LOADER.set(build(explicit));
}

/// Return the active language loader, lazily initializing with detected
/// languages if [`init`] was never called.
#[must_use]
pub fn loader() -> &'static FluentLanguageLoader {
    LOADER.get_or_init(|| build(None))
}

/// Build a loader for the negotiated locale, always loading the fallback so
/// every message id resolves even if a translation is incomplete.
fn build(explicit: Option<&str>) -> FluentLanguageLoader {
    let loader = fluent_language_loader!();

    if let Err(error) = loader.load_fallback_language(&Localizations) {
        // The fallback catalog is embedded and validated in CI, so this is
        // effectively unreachable; log rather than panic to honor lint policy.
        tracing::error!(%error, "failed to load fallback localization");
    }

    let requested = resolve_languages(explicit);
    if !requested.is_empty()
        && let Err(error) = i18n_embed::select(&loader, &Localizations, &requested)
    {
        tracing::warn!(%error, "failed to select requested localization; using fallback");
    }

    // Fluent wraps interpolated variables in Unicode bidi isolation marks by
    // default; they are invisible in browsers but show as stray characters in a
    // terminal. Disable them after the bundles are loaded so it applies to all.
    loader.set_use_isolating(false);

    loader
}

/// Resolve the ordered list of requested languages from the override, the
/// `TOCK_LANG` environment variable, or the operating-system locale.
fn resolve_languages(explicit: Option<&str>) -> Vec<LanguageIdentifier> {
    if let Some(tag) = explicit.map(str::trim).filter(|t| !t.is_empty()) {
        return parse_tag(tag).into_iter().collect();
    }

    if let Some(tag) = std::env::var(LANG_ENV)
        .ok()
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(ToOwned::to_owned)
    {
        return parse_tag(&tag).into_iter().collect();
    }

    DesktopLanguageRequester::new().requested_languages()
}

/// Parse a single BCP-47 language tag, logging and dropping invalid input.
fn parse_tag(tag: &str) -> Option<LanguageIdentifier> {
    match tag.parse::<LanguageIdentifier>() {
        Ok(id) => Some(id),
        Err(error) => {
            tracing::warn!(%tag, %error, "ignoring invalid language tag");
            None
        }
    }
}

/// Look up a localized message by id, returning the message id itself if it is
/// missing (so the macro path stays infallible). Prefer the [`tr!`](crate::tr)
/// macro, which adds compile-time id checking.
#[macro_export]
macro_rules! tr {
    ($message_id:literal) => {{
        i18n_embed_fl::fl!($crate::i18n::loader(), $message_id)
    }};
    ($message_id:literal, $($args:tt)*) => {{
        i18n_embed_fl::fl!($crate::i18n::loader(), $message_id, $($args)*)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_loads_and_resolves_known_message() {
        let loader = build(None);
        let msg = loader.get("error-prefix");
        assert!(msg.contains("error"));
    }

    #[test]
    fn explicit_unknown_locale_falls_back_to_english() {
        // A locale with no catalog must still resolve via the fallback.
        let loader = build(Some("zz-ZZ"));
        assert!(loader.get("error-prefix").contains("error"));
    }

    #[test]
    fn invalid_tag_is_ignored() {
        assert!(parse_tag("not a tag!!").is_none());
        assert!(parse_tag("es-ES").is_some());
    }

    #[test]
    fn current_language_reports_a_value() {
        let loader = build(None);
        assert!(!loader.current_language().to_string().is_empty());
    }
}
