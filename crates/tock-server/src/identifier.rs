//! Login-identifier canonicalization (issue #199, ADR-016 §5).
//!
//! Registration and login both key on a *normalized* form of the username so
//! that trivially-equivalent spellings (case, Unicode compatibility variants,
//! decomposed combining sequences) resolve to one account and cannot be used
//! to register a near-duplicate identity.
//!
//! ## Scheme
//!
//! `trim → NFKC → lowercase`:
//!
//! - **trim** drops surrounding ASCII/Unicode whitespace.
//! - **NFKC** (Normalization Form KC) folds compatibility variants — e.g.
//!   full-width `ａ` (U+FF41) → `a`, ligatures, and decomposed combining
//!   sequences (`e` + U+0301) into their composed canonical form (`é`).
//! - **lowercase** applies full Unicode case mapping so `Alice` and `alice`
//!   collide.
//!
//! ## Residual risk (documented, deliberately not solved here)
//!
//! NFKC + case folding does **not** collapse cross-script *homoglyphs*: the
//! Cyrillic `а` (U+0430) and the Latin `a` (U+0061) are distinct code points
//! that look identical but normalize to different strings. Defending against
//! confusable/spoofed identifiers is a separate concern (a confusables or
//! `skeleton` mapping, e.g. UTS #39) and is intentionally out of scope — adding
//! it here would be overreach and needs its own review. This function reduces
//! the *accidental* near-duplicate surface, not deliberate homoglyph spoofing.

use unicode_normalization::UnicodeNormalization;

/// Canonicalize a login identifier for storage, uniqueness, and lookup.
///
/// Applies `trim → NFKC → lowercase` (see the module docs). Pure and
/// allocation-only; safe to call on every register/login request.
#[must_use]
pub fn normalize(raw: &str) -> String {
    raw.trim().nfkc().collect::<String>().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(normalize("  alice  "), "alice");
        assert_eq!(normalize("\talice\n"), "alice");
    }

    #[test]
    fn folds_case() {
        assert_eq!(normalize("Alice"), "alice");
        assert_eq!(normalize("ALICE"), "alice");
        assert_eq!(normalize("aLiCe@Example.COM"), "alice@example.com");
    }

    #[test]
    fn folds_fullwidth_compatibility_variants() {
        // Full-width Latin letters (U+FF41..) normalize to ASCII under NFKC.
        let fullwidth = "\u{FF41}\u{FF4C}\u{FF49}\u{FF43}\u{FF45}"; // "ａｌｉｃｅ"
        assert_eq!(normalize(fullwidth), "alice");
    }

    #[test]
    fn composes_combining_sequences() {
        // "cafe" + U+0301 (combining acute) must equal precomposed "café".
        let decomposed = "cafe\u{0301}";
        let precomposed = "caf\u{00E9}";
        assert_eq!(normalize(decomposed), normalize(precomposed));
    }

    #[test]
    fn distinct_names_stay_distinct() {
        assert_ne!(normalize("alice"), normalize("bob"));
    }

    #[test]
    fn cross_script_homoglyphs_are_not_collapsed() {
        // Documented residual risk: Cyrillic 'а' (U+0430) != Latin 'a' (U+0061).
        // NFKC + case folding leaves these distinct on purpose.
        let cyrillic = "\u{0430}lice";
        assert_ne!(normalize(cyrillic), normalize("alice"));
    }

    #[test]
    fn is_idempotent() {
        let once = normalize("  ＡＬＩＣＥ@Example.com  ");
        assert_eq!(normalize(&once), once);
    }
}
