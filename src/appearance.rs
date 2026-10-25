//! The vault's Obsidian appearance: active theme, enabled CSS snippets, accent
//! colour, fonts and light/dark mode, read from `.obsidian/appearance.json`.

use crate::config::SiteConfig;
use anyhow::{Result, bail};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::Path;

pub struct Appearance {
    /// Contents of `_site/theme.css`; empty when there's nothing to apply.
    pub css: String,
    /// Initial colour mode for visitors who haven't toggled it: "system", "dark" or "light".
    pub mode: String,
    pub warnings: Vec<String>,
}

pub fn load(vault: &Path, site: &SiteConfig) -> Result<Appearance> {
    let obsidian = vault.join(".obsidian");
    let json: Value = std::fs::read_to_string(obsidian.join("appearance.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let get = |k: &str| json.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
    let mut warnings = Vec::new();

    let mode = match site.color_mode.as_deref() {
        Some(m @ ("system" | "dark" | "light")) => m.to_string(),
        Some(other) => bail!("site.color_mode must be \"system\", \"dark\" or \"light\", not \"{other}\""),
        None => match get("theme") {
            Some("obsidian") => "dark".into(),
            Some("moonstone") => "light".into(),
            _ => "system".into(),
        },
    };

    // Variables Obsidian derives from appearance settings; emitted before the
    // theme so a theme's own values still win.
    let mut vars = String::new();
    if let Some((h, s, l)) = get("accentColor").and_then(hex_to_hsl) {
        write!(vars, "--accent-h:{h};--accent-s:{s}%;--accent-l:{l}%;").unwrap();
    }
    if let Some(f) = get("textFontFamily") {
        write!(vars, "--font-text-override:{f};").unwrap();
    }
    if let Some(f) = get("monospaceFontFamily") {
        write!(vars, "--font-monospace-override:{f};").unwrap();
    }
    if let Some(size) = json.get("baseFontSize").and_then(Value::as_f64) {
        write!(vars, "--font-text-size:{size}px;").unwrap();
    }

    let mut css = String::new();
    if !vars.is_empty() {
        writeln!(css, "/* appearance.json */\nbody{{{vars}}}").unwrap();
    }

    let theme = match site.theme.as_deref() {
        Some("none") => None,
        Some("auto") | None => get("cssTheme").map(str::to_string),
        Some(name) => Some(name.to_string()),
    };
    if let Some(name) = theme {
        let candidates = [obsidian.join("themes").join(&name).join("theme.css"), obsidian.join("themes").join(format!("{name}.css"))];
        match candidates.iter().find(|p| p.is_file()) {
            Some(path) => {
                let text = std::fs::read_to_string(path)?;
                writeln!(css, "/* theme: {name} */\n{text}").unwrap();
                // Before Obsidian 1.13 callout colours were "r, g, b" triplets.
                if text.contains("rgb(var(--callout-color") {
                    writeln!(css, "/* vault-web-gen: triplet callout colours */\n.callout{{--vw-callout:rgb(var(--callout-color))}}").unwrap();
                }
            }
            None if site.theme.is_some() && site.theme.as_deref() != Some("auto") => {
                bail!("theme '{name}' not found in {}", obsidian.join("themes").display())
            }
            None => warnings.push(format!("active theme '{name}' not found in .obsidian/themes; using the default look")),
        }
    }

    let snippets: Vec<String> = match &site.snippets {
        Some(list) => list.clone(),
        None if site.theme.as_deref() == Some("none") => vec![],
        None => json
            .get("enabledCssSnippets")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default(),
    };
    for name in snippets {
        let path = obsidian.join("snippets").join(format!("{}.css", name.trim_end_matches(".css")));
        match std::fs::read_to_string(&path) {
            Ok(text) => writeln!(css, "/* snippet: {name} */\n{text}").unwrap(),
            Err(_) if site.snippets.is_some() => bail!("snippet '{name}' not found at {}", path.display()),
            Err(_) => warnings.push(format!("enabled snippet '{name}' not found in .obsidian/snippets")),
        }
    }

    // `@charset` is only valid as the very first rule of a stylesheet.
    if !css.is_empty() {
        static CHARSET: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r#"@charset\s+"[^"]*"\s*;"#).unwrap());
        css = format!("@charset \"UTF-8\";\n{}", CHARSET.replace_all(&css, ""));
    }
    Ok(Appearance { css, mode, warnings })
}

fn hex_to_hsl(hex: &str) -> Option<(i64, i64, i64)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|v| v as f64 / 255.0);
    let (r, g, b) = (channel(0)?, channel(2)?, channel(4)?);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let (h, s) = if d == 0.0 {
        (0.0, 0.0)
    } else {
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            60.0 * (((g - b) / d) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        (if h < 0.0 { h + 360.0 } else { h }, s)
    };
    Some((h.round() as i64, (s * 100.0).round() as i64, (l * 100.0).round() as i64))
}
