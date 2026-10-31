//! `nix search --json`, wrapped with query escaping and our own ranking.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{rank, run};

#[derive(Debug, Clone)]
pub struct SearchHit {
    /// Attribute path inside nixpkgs, e.g. `ripgrep` or `python3Packages.requests`.
    pub attr: String,
    pub version: String,
    pub description: String,
    pub score: i64,
}

#[derive(Deserialize)]
struct RawHit {
    #[serde(default)]
    pname: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
}

/// Search `flake_ref` for `query` (space-separated terms, all must match) and
/// return at most `limit` hits, best first.
pub async fn search(flake_ref: &str, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let terms = rank::terms(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let escaped: Vec<String> = terms.iter().map(|t| rank::escape_regex(t)).collect();
    let mut args: Vec<&str> = vec!["search", flake_ref, "--json"];
    args.extend(escaped.iter().map(String::as_str));

    let out = match run(&args).await {
        Ok(out) => out,
        // Some Nix versions exit non-zero when nothing matches.
        Err(e) if e.to_string().contains("no results") => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let raw: BTreeMap<String, RawHit> =
        serde_json::from_str(&out.stdout).context("could not parse `nix search --json` output")?;

    let mut hits: Vec<SearchHit> = raw
        .into_iter()
        .map(|(key, r)| {
            let attr = attr_from_key(&key);
            let score = rank::score(&terms, &attr, &r.pname, &r.description);
            SearchHit {
                attr,
                version: r.version,
                description: r.description,
                score,
            }
        })
        .collect();

    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.attr.cmp(&b.attr)));
    hits.truncate(limit);
    Ok(hits)
}

/// `legacyPackages.x86_64-linux.python3Packages.requests` -> `python3Packages.requests`.
fn attr_from_key(key: &str) -> String {
    key.splitn(3, '.').nth(2).unwrap_or(key).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_flake_output_and_system() {
        assert_eq!(
            attr_from_key("legacyPackages.x86_64-linux.ripgrep"),
            "ripgrep"
        );
        assert_eq!(
            attr_from_key("legacyPackages.aarch64-darwin.python3Packages.requests"),
            "python3Packages.requests"
        );
    }
}
