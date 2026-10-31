//! `nix profile` operations. Requires Nix >= 2.20 (JSON v3, name-keyed elements).

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::run;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Element name in the profile (what `nix profile remove <name>` expects).
    pub name: String,
    pub version: Option<String>,
    /// Where it came from, e.g. `flake:nixpkgs#ripgrep`.
    pub origin: Option<String>,
    pub active: bool,
}

#[derive(Deserialize)]
struct ProfileJson {
    elements: BTreeMap<String, Element>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Element {
    #[serde(default = "default_true")]
    active: bool,
    #[serde(default)]
    store_paths: Vec<String>,
    original_url: Option<String>,
    attr_path: Option<String>,
}

fn default_true() -> bool {
    true
}

pub fn parse(json: &str) -> Result<Vec<Package>> {
    let parsed: ProfileJson = serde_json::from_str(json)
        .context("unexpected `nix profile list --json` output (Nix >= 2.20 required)")?;

    Ok(parsed
        .elements
        .into_iter()
        .map(|(name, el)| {
            let version = el
                .store_paths
                .first()
                .and_then(|p| version_from_store_path(&name, p));
            let origin = match (el.original_url, el.attr_path) {
                (Some(url), Some(attr)) => Some(format!("{url}#{}", strip_system(&attr))),
                (Some(url), None) => Some(url),
                _ => None,
            };
            Package {
                name,
                version,
                origin,
                active: el.active,
            }
        })
        .collect())
}

/// `/nix/store/<hash>-ripgrep-14.1.0` + name `ripgrep` -> `14.1.0`.
fn version_from_store_path(name: &str, store_path: &str) -> Option<String> {
    let file = store_path.rsplit('/').next()?;
    let (_hash, rest) = file.split_once('-')?;
    rest.strip_prefix(name)?
        .strip_prefix('-')
        .map(str::to_owned)
}

/// `packages.x86_64-linux.hello` -> `hello`.
fn strip_system(attr: &str) -> &str {
    attr.splitn(3, '.').nth(2).unwrap_or(attr)
}

pub async fn list() -> Result<Vec<Package>> {
    let out = run(&["profile", "list", "--json"]).await?;
    parse(&out.stdout)
}

pub async fn install(flake_ref: &str, attr: &str) -> Result<()> {
    let target = format!("{flake_ref}#{attr}");
    run(&["profile", "install", target.as_str()])
        .await
        .map(drop)
}

pub async fn upgrade(name: &str) -> Result<()> {
    run(&["profile", "upgrade", name]).await.map(drop)
}

pub async fn upgrade_all() -> Result<()> {
    run(&["profile", "upgrade", "--all"]).await.map(drop)
}

pub async fn remove(name: &str) -> Result<()> {
    run(&["profile", "remove", name]).await.map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real output of `nix profile list --json` (Nix 2.34).
    const FIXTURE: &str = r#"{"elements":{"nix":{"active":true,"priority":5,"storePaths":["/nix/store/q7f0d4m54yj98fcjmbkscw83j82fypnd-nix-2.34.6"]},"nss-cacert-static-x86_64-unknown-linux-musl":{"active":true,"priority":5,"storePaths":["/nix/store/5jpvc9pzlr75vnjmz2xp9h8fqwy2hadf-nss-cacert-static-x86_64-unknown-linux-musl-3.117"]}},"version":3}"#;

    #[test]
    fn parses_real_profile_output() {
        let pkgs = parse(FIXTURE).unwrap();
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "nix");
        assert_eq!(pkgs[0].version.as_deref(), Some("2.34.6"));
        assert_eq!(pkgs[1].version.as_deref(), Some("3.117"));
        assert!(pkgs[0].active);
    }

    #[test]
    fn builds_origin_from_flake_fields() {
        let json = r#"{"elements":{"hello":{"active":true,"originalUrl":"flake:nixpkgs","attrPath":"legacyPackages.x86_64-linux.hello","storePaths":["/nix/store/aaaa-hello-2.12"]}},"version":3}"#;
        let pkgs = parse(json).unwrap();
        assert_eq!(pkgs[0].origin.as_deref(), Some("flake:nixpkgs#hello"));
        assert_eq!(pkgs[0].version.as_deref(), Some("2.12"));
    }

    #[test]
    fn version_is_none_when_name_differs() {
        assert_eq!(
            version_from_store_path("foo", "/nix/store/aaaa-bar-1.0"),
            None
        );
    }
}
