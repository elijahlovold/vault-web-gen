//! `vault-web.toml`: the export definition that lives in the vault root.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const CONFIG_NAME: &str = "vault-web.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub site: SiteConfig,
    /// Exported at the output root. Optional; without it the root only gets a blank index.
    pub public: Option<SliceConfig>,
    /// Private slices, each exported to `<output>/<id>/`.
    #[serde(default, rename = "slice")]
    pub slices: Vec<SliceConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteConfig {
    /// Directory that maps to the webserver's `/vault/`. Relative to the vault root.
    pub output: Option<PathBuf>,
    /// Only used to print share links, e.g. "https://example.com/vault/".
    pub base_url: Option<String>,
    /// Frontmatter tag every exported note must carry.
    #[serde(default = "default_require_tag")]
    pub require_tag: String,
    /// Globs (vault-relative) that are never exported, in addition to dot-folders.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Frontmatter keys not shown in the properties table.
    #[serde(default)]
    pub hide_properties: Vec<String>,
    /// Load KaTeX / Mermaid from jsDelivr on pages that need them.
    #[serde(default = "yes")]
    pub cdn: bool,
}

impl Default for SiteConfig {
    fn default() -> Self {
        SiteConfig {
            output: None,
            base_url: None,
            require_tag: default_require_tag(),
            exclude: vec![],
            hide_properties: vec![],
            cdn: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SliceConfig {
    /// Local label for the slice; never published.
    pub name: Option<String>,
    /// UUID path segment. Generated and written back into the config on first build.
    pub id: Option<String>,
    /// Site title shown in the sidebar and browser tab.
    pub title: Option<String>,
    pub password: Option<String>,
    /// Read the password from this environment variable instead.
    pub password_env: Option<String>,
    /// Note rendered as the slice's index.html (link syntax, e.g. "Home" or "folder/Home").
    pub home: Option<String>,
    /// Vault-relative globs, e.g. "reflections/**".
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Notes carrying any of these frontmatter tags (or their nested children) are included.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Individual notes, by name or path, as you'd write them in a wikilink.
    #[serde(default)]
    pub notes: Vec<String>,
}

fn default_require_tag() -> String {
    "shared".into()
}
fn yes() -> bool {
    true
}

impl SliceConfig {
    pub fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| "public".into())
    }

    pub fn resolve_password(&self) -> Result<Option<String>> {
        match (&self.password, &self.password_env) {
            (Some(_), Some(_)) => bail!("slice '{}': set either password or password_env, not both", self.label()),
            (Some(p), None) => Ok(Some(p.clone())),
            (None, Some(var)) => std::env::var(var)
                .map(Some)
                .with_context(|| format!("slice '{}': env var {var} is not set", self.label())),
            (None, None) => Ok(None),
        }
    }
}

pub struct LoadedConfig {
    pub config: Config,
    pub path: PathBuf,
}

impl LoadedConfig {
    pub fn base_dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }
}

/// Loads the config, assigning UUIDs to slices that lack one and persisting them
/// back into the file (comments and formatting are preserved).
pub fn load(path: &Path, assign_ids: bool) -> Result<LoadedConfig> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut config: Config = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;

    if config.slices.iter().any(|s| s.id.is_none()) && assign_ids {
        let mut doc: toml_edit::DocumentMut = text.parse()?;
        let tables = doc["slice"].as_array_of_tables_mut().context("[[slice]] must be an array of tables")?;
        for (slice, table) in config.slices.iter_mut().zip(tables.iter_mut()) {
            if slice.id.is_none() {
                let id = uuid::Uuid::new_v4().to_string();
                eprintln!("assigned id {id} to slice '{}'", slice.label());
                table.insert("id", toml_edit::value(id.clone()));
                slice.id = Some(id);
            }
        }
        std::fs::write(path, doc.to_string()).with_context(|| format!("writing ids to {}", path.display()))?;
    }

    validate(&config)?;
    Ok(LoadedConfig { config, path: path.to_path_buf() })
}

fn validate(config: &Config) -> Result<()> {
    if let Some(p) = &config.public {
        if p.password.is_some() || p.password_env.is_some() {
            bail!("[public] cannot have a password");
        }
        if p.id.is_some() {
            bail!("[public] is exported at the output root and takes no id");
        }
    }
    let mut names = HashSet::new();
    let mut ids = HashSet::new();
    for s in &config.slices {
        let Some(name) = &s.name else { bail!("every [[slice]] needs a name") };
        if !names.insert(name.clone()) {
            bail!("duplicate slice name '{name}'");
        }
        if let Some(id) = &s.id {
            uuid::Uuid::parse_str(id).with_context(|| format!("slice '{name}': id '{id}' is not a UUID"))?;
            if !ids.insert(id.clone()) {
                bail!("duplicate slice id '{id}'");
            }
        }
    }
    Ok(())
}

pub const SAMPLE: &str = r#"# vault-web-gen export definition.
# Every exported note must carry the `require_tag` tag in its frontmatter.

[site]
output = "../vault-site"          # copy this dir to the webserver's /vault/
# base_url = "https://example.com/vault/"   # only used to print share links
require_tag = "shared"
exclude = ["templates/**"]
hide_properties = []
cdn = true                        # KaTeX / Mermaid from jsDelivr when a page needs them

# Exported at /vault/. Never links to any slice.
[public]
title = "Notes"
# home = "Home"
include = []
tags = ["public"]

# Each slice is exported to /vault/<id>/ and is fully self-contained.
# `id` is generated on the first build; keep it stable or links break.
# [[slice]]
# name = "alice"
# title = "Notes for Alice"
# password = "correct horse battery staple"   # or password_env = "ALICE_PW"
# include = ["projects/foo/**"]
# tags = ["share/alice"]
# notes = ["Some Note"]
"#;
