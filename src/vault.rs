//! Vault scanning and Obsidian-style link resolution.

use anyhow::Result;
use serde_yaml::{Mapping, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct Note {
    /// Vault-relative path with `/` separators, including `.md`.
    pub rel: String,
    pub stem: String,
    pub frontmatter: Option<Mapping>,
    /// Frontmatter tags, lowercased, without `#`.
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    /// Markdown after the frontmatter.
    pub body: String,
}

pub struct Vault {
    pub root: PathBuf,
    pub notes: Vec<Note>,
    /// Vault-relative paths of non-markdown files.
    pub attachments: Vec<String>,
    by_stem: HashMap<String, Vec<usize>>,
    by_alias: HashMap<String, Vec<usize>>,
    attachments_by_name: HashMap<String, Vec<usize>>,
}

impl Vault {
    pub fn scan(root: &Path) -> Result<Vault> {
        let mut notes = Vec::new();
        let mut attachments = Vec::new();
        let walker = WalkDir::new(root).sort_by_file_name().into_iter().filter_entry(|e| {
            e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.')
        });
        for entry in walker {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = rel_path(root, entry.path());
            if rel.to_lowercase().ends_with(".md") {
                let text = std::fs::read_to_string(entry.path())?;
                notes.push(Note::parse(rel, &text));
            } else {
                attachments.push(rel);
            }
        }

        let mut by_stem: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_alias: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, n) in notes.iter().enumerate() {
            by_stem.entry(n.stem.to_lowercase()).or_default().push(i);
            for a in &n.aliases {
                by_alias.entry(a.to_lowercase()).or_default().push(i);
            }
        }
        let mut attachments_by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, a) in attachments.iter().enumerate() {
            attachments_by_name.entry(file_name(a).to_lowercase()).or_default().push(i);
        }

        Ok(Vault { root: root.to_path_buf(), notes, attachments, by_stem, by_alias, attachments_by_name })
    }

    /// Resolves the note part of a link (`Name`, `folder/Name`, `Name.md`) the way
    /// Obsidian does: by name anywhere in the vault, preferring the linking note's
    /// folder, then the shortest path.
    pub fn resolve_note(&self, link: &str, from: Option<usize>) -> Option<usize> {
        let link = percent_decode(link.trim());
        let link = link.strip_suffix(".md").unwrap_or(&link).trim_start_matches('/');
        if link.is_empty() {
            return None;
        }
        let lower = link.to_lowercase();
        let name = lower.rsplit('/').next().unwrap();
        let candidates: Vec<usize> = self
            .by_stem
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&i| {
                let path = self.notes[i].rel.to_lowercase();
                let path = path.strip_suffix(".md").unwrap();
                path == lower || path.ends_with(&format!("/{lower}"))
            })
            .collect();
        if let Some(best) = self.pick(&candidates, from, |i| &self.notes[i].rel) {
            return Some(best);
        }
        let aliased: Vec<usize> = self.by_alias.get(&lower).cloned().unwrap_or_default();
        self.pick(&aliased, from, |i| &self.notes[i].rel)
    }

    pub fn resolve_attachment(&self, link: &str, from: Option<usize>) -> Option<usize> {
        let link = percent_decode(link.trim());
        let link = link.trim_start_matches("./").trim_start_matches('/');
        let lower = link.to_lowercase();
        // Relative to the linking note first (standard markdown links).
        if let Some(from) = from {
            let dir = parent(&self.notes[from].rel);
            let joined = normalize(&if dir.is_empty() { lower.clone() } else { format!("{}/{lower}", dir.to_lowercase()) });
            if let Some(i) = self.attachments.iter().position(|a| a.to_lowercase() == joined) {
                return Some(i);
            }
        }
        let name = file_name(&lower);
        let candidates: Vec<usize> = self
            .attachments_by_name
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&i| {
                let path = self.attachments[i].to_lowercase();
                path == lower || path.ends_with(&format!("/{lower}"))
            })
            .collect();
        self.pick(&candidates, from, |i| &self.attachments[i])
    }

    fn pick<'a>(&'a self, candidates: &[usize], from: Option<usize>, path: impl Fn(usize) -> &'a str) -> Option<usize> {
        let from_dir = from.map(|f| parent(&self.notes[f].rel));
        candidates.iter().copied().min_by_key(|&i| {
            let p = path(i);
            let same_dir = from_dir == Some(parent(p));
            (!same_dir, p.matches('/').count(), p.len())
        })
    }
}

impl Note {
    fn parse(rel: String, text: &str) -> Note {
        let stem = file_name(&rel);
        let stem = stem[..stem.len() - 3].to_string();
        let (yaml, body) = split_frontmatter(text);
        let frontmatter = yaml.and_then(|y| match serde_yaml::from_str::<Value>(y) {
            Ok(Value::Mapping(m)) => Some(m),
            Ok(Value::Null) => None,
            Ok(_) => None,
            Err(e) => {
                eprintln!("warning: {rel}: invalid frontmatter ({e}); treating as untagged");
                None
            }
        });
        let tags = frontmatter
            .as_ref()
            .map(|m| ["tags", "tag"].iter().flat_map(|k| string_list(m.get(*k))).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.trim().trim_start_matches('#').to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        let aliases = frontmatter
            .as_ref()
            .map(|m| ["aliases", "alias"].iter().flat_map(|k| string_list(m.get(*k))).collect())
            .unwrap_or_default();
        Note { rel, stem, frontmatter, tags, aliases, body: body.to_string() }
    }

    pub fn has_tag(&self, tag: &str) -> bool {
        let tag = tag.trim_start_matches('#').to_lowercase();
        self.tags.iter().any(|t| *t == tag)
    }

    /// Tag search semantics: `share` also matches `share/alice`.
    pub fn has_tag_or_child(&self, tag: &str) -> bool {
        let tag = tag.trim_start_matches('#').to_lowercase();
        self.tags.iter().any(|t| *t == tag || t.starts_with(&format!("{tag}/")))
    }

    pub fn folder(&self) -> &str {
        parent(&self.rel)
    }
}

/// Accepts a YAML list or a comma/space separated string.
fn string_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Sequence(s)) => s.iter().filter_map(scalar_string).collect(),
        Some(Value::String(s)) => s.split([',', ' ']).filter(|t| !t.is_empty()).map(str::to_string).collect(),
        _ => vec![],
    }
}

pub fn scalar_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return (None, text);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" || line.trim_end() == "..." {
            return (Some(&rest[..offset]), &rest[offset + line.len()..]);
        }
        offset += line.len();
    }
    (None, text)
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap()
}

pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    out.join("/")
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}
