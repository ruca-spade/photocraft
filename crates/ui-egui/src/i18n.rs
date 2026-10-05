//! Minimal UI localisation: English source strings are looked up in a bundled dictionary and
//! shown in the selected language; anything missing falls back to the English text unchanged.
//!
//! The English string stays the identity used by logic (command ids, menu paths, de-duplication);
//! translation happens only where text is drawn. Language selection: `PHOTOCRAFT_LANG`
//! (`ja` / `en`), otherwise the OS locale (`LC_ALL`, `LC_MESSAGES`, `LANG`); default English.
//! Dictionary format: `assets/i18n/ja.tsv` (`English<TAB>translation`, `#` comments).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Ja,
}

static JA_TSV: &str = include_str!("../../../assets/i18n/ja.tsv");

/// Parse a dictionary. The first entry for a key wins; blank lines, comments and malformed lines are skipped.
pub fn parse_dict(src: &str) -> HashMap<&str, &str> {
    let mut m = HashMap::new();
    for line in src.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('\t')
            && !k.is_empty()
            && !v.is_empty()
        {
            m.entry(k).or_insert(v);
        }
    }
    m
}

/// Pick a language from a `PHOTOCRAFT_LANG`-style value or a POSIX locale such as `ja_JP.UTF-8`.
pub fn parse_lang(s: &str) -> Option<Lang> {
    let s = s.trim().to_ascii_lowercase();
    if s.starts_with("ja") {
        Some(Lang::Ja)
    } else if s.starts_with("en") || s == "c" || s == "posix" {
        Some(Lang::En)
    } else {
        None
    }
}

fn detect() -> Lang {
    #[cfg(not(target_arch = "wasm32"))]
    {
        for var in ["PHOTOCRAFT_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Some(l) = std::env::var(var).ok().and_then(|v| parse_lang(&v)) {
                return l;
            }
        }
    }
    Lang::En
}

/// The active UI language (decided once per process).
pub fn lang() -> Lang {
    static L: OnceLock<Lang> = OnceLock::new();
    *L.get_or_init(detect)
}

fn ja() -> &'static HashMap<&'static str, &'static str> {
    static D: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    D.get_or_init(|| parse_dict(JA_TSV))
}

/// Look `s` up in `dict`; a trailing `…` is ignored for the lookup and re-appended to the result.
pub fn lookup<'a>(dict: &HashMap<&str, &'a str>, s: &str) -> Option<Cow<'a, str>> {
    if let Some(v) = dict.get(s) {
        return Some(Cow::Borrowed(v));
    }
    let base = s.strip_suffix('…')?;
    dict.get(base).map(|v| Cow::Owned(format!("{v}…")))
}

/// Translate an English UI string for display. Unknown strings (and English mode) come back unchanged.
pub fn tr(s: &str) -> Cow<'_, str> {
    if lang() == Lang::En {
        return Cow::Borrowed(s);
    }
    match lookup(ja(), s) {
        Some(Cow::Borrowed(v)) => Cow::Borrowed(v),
        Some(Cow::Owned(v)) => Cow::Owned(v),
        None => Cow::Borrowed(s),
    }
}

/// [`tr`] as an owned `String`, for egui widget arguments (`ui.label(ts("Opacity"))`).
pub fn ts(s: &str) -> String {
    tr(s).into_owned()
}

/// Translate a `{}` template, then substitute the arguments in order
/// (`trf("Resolution: {} pixels/inch", &[&dpi])`). Missing translation = English template.
pub fn trf(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let t = tr(template);
    let mut out = String::with_capacity(t.len() + 8 * args.len());
    let mut it = args.iter();
    let mut rest: &str = &t;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(a) = it.next() {
            out.push_str(&a.to_string());
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_is_well_formed() {
        for (n, line) in JA_TSV.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            assert_eq!(parts.len(), 2, "line {}: expected key<TAB>value: {line:?}", n + 1);
            assert!(!parts[0].ends_with('…'), "line {}: keys omit the trailing ellipsis", n + 1);
            assert!(!parts[1].is_empty());
        }
        assert!(parse_dict(JA_TSV).len() > 500);
    }

    #[test]
    fn lookup_handles_ellipsis_and_unknowns() {
        let d = parse_dict(JA_TSV);
        assert_eq!(lookup(&d, "Save As…").as_deref(), Some("別名で保存…"));
        assert_eq!(lookup(&d, "Save").as_deref(), Some("保存"));
        assert!(lookup(&d, "No Such Menu Item").is_none());
    }

    #[test]
    fn every_catalog_label_is_translated() {
        let d = parse_dict(JA_TSV);
        let mut missing = Vec::new();
        for &(path, label, _, _) in crate::menu_catalog::CATALOG {
            for s in path.iter().copied().chain(std::iter::once(label)) {
                if s != "---" && lookup(&d, s).is_none() {
                    missing.push(s);
                }
            }
        }
        missing.sort_unstable();
        missing.dedup();
        assert!(missing.is_empty(), "untranslated menu strings: {missing:?}");
    }

    #[test]
    fn trf_substitutes_in_order() {
        // English mode or unknown template: plain substitution.
        assert_eq!(trf("{} of {} done", &[&1, &2]), "1 of 2 done");
    }

    #[test]
    fn parses_locales() {
        assert_eq!(parse_lang("ja_JP.UTF-8"), Some(Lang::Ja));
        assert_eq!(parse_lang("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(parse_lang("de_DE"), None);
    }
}
