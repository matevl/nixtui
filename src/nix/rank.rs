//! Relevance ranking for `nix search` results.
//!
//! `nix search` returns matches in alphabetical order, which buries the package
//! you actually want ("ripgrep" lands after "ripgrep-all", and nested attrs like
//! `python3Packages.*` flood the list). This module is pure logic with no
//! dependencies, so it is cheap to test and to tune.

/// Escape a user-typed term: `nix search` takes regexes, we want literal text.
pub fn escape_regex(term: &str) -> String {
    let mut out = String::with_capacity(term.len() + 2);
    for (i, c) in term.chars().enumerate() {
        let special = matches!(
            c,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        );
        // A leading '-' would be parsed by nix as a command-line flag.
        if special || (i == 0 && c == '-') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Split a query into lowercase search terms (all of them must match).
pub fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// Higher is better. `attr` is the attribute without the `legacyPackages.<system>.` prefix.
pub fn score(terms: &[String], attr: &str, pname: &str, description: &str) -> i64 {
    let attr = attr.to_lowercase();
    let leaf = attr.rsplit('.').next().unwrap_or(&attr).to_string();
    let pname = pname.to_lowercase();
    let desc = description.to_lowercase();

    let mut total = 0i64;
    for t in terms {
        let t = t.as_str();
        let by_name = if leaf == t || pname == t {
            1000
        } else if leaf.starts_with(t) || pname.starts_with(t) {
            600
        } else if word_start(&leaf, t) || word_start(&pname, t) {
            400
        } else if leaf.contains(t) || pname.contains(t) {
            300
        } else {
            0
        };
        let by_desc = if has_word(&desc, t) {
            120
        } else if desc.contains(t) {
            60
        } else {
            0
        };
        total += by_name.max(by_desc);
    }

    if total == 0 {
        return 0;
    }
    // Nested sets (python3Packages.x, haskellPackages.x, ...) are rarely what you want.
    if attr.contains('.') {
        total -= 150;
    }
    // Tie-break: shorter names are closer to what was typed.
    total - (leaf.len() as i64).min(60)
}

/// Does any `-`/`_`/`.`-separated word of `haystack` start with `needle`?
fn word_start(haystack: &str, needle: &str) -> bool {
    haystack
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(needle))
}

/// Does `haystack` contain `needle` as a whole word?
fn has_word(haystack: &str, needle: &str) -> bool {
    haystack
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(q: &str, attr: &str, pname: &str, desc: &str) -> i64 {
        score(&terms(q), attr, pname, desc)
    }

    #[test]
    fn exact_beats_prefix_beats_substring() {
        let exact = s("ripgrep", "ripgrep", "ripgrep", "fast grep");
        let prefix = s("ripgrep", "ripgrep-all", "ripgrep-all", "rg wrapper");
        let infix = s("ripgrep", "xripgrepx", "xripgrepx", "");
        assert!(exact > prefix, "{exact} > {prefix}");
        assert!(prefix > infix, "{prefix} > {infix}");
    }

    #[test]
    fn nested_attrs_are_demoted() {
        let top = s("requests", "requests", "requests", "");
        let nested = s("requests", "python3Packages.requests", "requests", "");
        assert!(top > nested);
    }

    #[test]
    fn name_hit_beats_description_hit() {
        let by_name = s("editor", "editor-foo", "editor-foo", "");
        let by_desc = s("editor", "vim", "vim", "Highly configurable text editor");
        assert!(by_name > by_desc);
    }

    #[test]
    fn multiple_terms_accumulate() {
        let one = s("text", "foo", "foo", "a text tool");
        let two = s("text editor", "foo", "foo", "a text editor");
        assert!(two > one);
    }

    #[test]
    fn no_match_scores_zero() {
        assert_eq!(s("zzz", "foo", "foo", "bar"), 0);
    }

    #[test]
    fn escapes_regex_metacharacters() {
        assert_eq!(escape_regex("c++"), r"c\+\+");
        assert_eq!(escape_regex("a.b"), r"a\.b");
        assert_eq!(escape_regex("-foo"), r"\-foo");
        assert_eq!(escape_regex("plain-name"), "plain-name");
    }

    #[test]
    fn terms_are_lowercased_and_split() {
        assert_eq!(terms("  Text   Editor "), vec!["text", "editor"]);
    }
}
