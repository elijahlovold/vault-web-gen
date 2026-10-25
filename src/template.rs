//! The HTML shell shared by every page.

use crate::render::{ROOT, esc};
use std::fmt::Write as _;

pub struct Page<'a> {
    pub site_title: &'a str,
    pub title: &'a str,
    /// Output path relative to the slice root, e.g. "folder/note.html".
    pub path: &'a str,
    /// Path of the note shown (differs from `path` for a home note shown as index.html).
    pub note_path: Option<&'a str>,
    pub article: &'a str,
    pub toc: &'a str,
    pub math: bool,
    pub mermaid: bool,
    pub cdn: bool,
    /// Whether `_site/theme.css` (Obsidian theme + snippets) exists.
    pub theme_css: bool,
    /// Initial colour mode: "system", "dark" or "light".
    pub mode: &'a str,
    pub private: bool,
    pub encryption: Option<Encryption<'a>>,
}

pub struct Encryption<'a> {
    pub slice_id: &'a str,
    pub salt_b64: &'a str,
    pub iterations: u32,
    /// base64(nonce || ciphertext) of the JSON {title, article, toc}.
    pub payload_b64: String,
}

pub const ICON: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Cpath d='M50 6 88 28v44L50 94 12 72V28z' fill='%237c5cdb'/%3E%3C/svg%3E";

pub fn root_prefix(path: &str) -> String {
    "../".repeat(path.matches('/').count())
}

/// Swaps the [`ROOT`] placeholder for the page's relative path to the slice root.
pub fn relativize(html: &str, path: &str) -> String {
    html.replace(ROOT, &root_prefix(path))
}

pub fn article_html(title: &str, properties: &str, body: &str, backlinks: &str) -> String {
    format!(
        r#"<article class="markdown-preview-view markdown-rendered"><h1 class="page-title inline-title">{}</h1>{properties}{body}{backlinks}</article>"#,
        esc(title)
    )
}

pub fn toc_html(headings: &[crate::render::Heading]) -> String {
    let mut s = String::new();
    let min = headings.iter().map(|h| h.level).min().unwrap_or(1);
    for h in headings {
        write!(
            s,
            r##"<a class="toc-item" style="--depth:{}" href="#{}">{}</a>"##,
            h.level - min,
            esc(&h.id),
            esc(&h.text)
        )
        .unwrap();
    }
    s
}

pub fn render(p: &Page) -> String {
    let root = root_prefix(p.path);
    let mut head_meta = String::new();
    if p.private {
        head_meta.push_str(r#"<meta name="robots" content="noindex, nofollow"><meta name="referrer" content="no-referrer">"#);
    }
    let (title, article, toc) = match &p.encryption {
        Some(_) => (p.site_title.to_string(), lock_html(p.site_title), String::new()),
        None => (format!("{} · {}", p.title, p.site_title), p.article.to_string(), p.toc.to_string()),
    };
    let config = format!(
        r#"{{"root":{},"page":{},"note":{},"math":{},"mermaid":{},"cdn":{},"encrypted":{}}}"#,
        json(&root),
        json(p.path),
        p.note_path.map(json).unwrap_or_else(|| "null".into()),
        p.math,
        p.mermaid,
        p.cdn,
        match &p.encryption {
            Some(e) => format!(
                r#"{{"slice":{},"salt":{},"iterations":{}}}"#,
                json(e.slice_id),
                json(e.salt_b64),
                e.iterations
            ),
            None => "null".into(),
        }
    );
    // Index pages are reachable as a directory URL. Without the trailing slash
    // ("/vault/<id>") every relative link and asset would resolve one level up,
    // into the parent slice, so add the slash before anything else loads.
    let slash_fix = if p.path == "index.html" {
        r#"<script>(function(l){if(!/\/$|\.html$/.test(l.pathname))l.replace(l.pathname+"/"+l.search+l.hash)})(location)</script>
"#
    } else {
        ""
    };
    let payload = p
        .encryption
        .as_ref()
        .map(|e| format!(r#"<script type="application/octet-stream" id="vwg-payload">{}</script>"#, e.payload_b64))
        .unwrap_or_default();

    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
{slash_fix}<meta name="viewport" content="width=device-width, initial-scale=1">
{head_meta}
<title>{title}</title>
<link rel="icon" href="{ICON}">
<script>(function(){{var t;try{{t=localStorage.getItem("vwg-theme")}}catch(e){{}}if(!t)t={mode}==="system"?(matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light"):{mode};document.documentElement.dataset.theme=t}})()</script>
<link rel="stylesheet" href="{root}_site/style.css">
<link rel="stylesheet" href="{root}_site/hl-light.css" id="hl-light">
<link rel="stylesheet" href="{root}_site/hl-dark.css" id="hl-dark">
{theme_link}</head>
<body>
<script>document.body.classList.add("theme-"+document.documentElement.dataset.theme)</script>
<div class="app">
<aside class="sidebar sidebar-left" id="sidebar-left">
  <div class="site-title"><a href="{root}index.html">{site_title}</a></div>
  <div class="search-box"><input type="search" id="search-input" placeholder="Search… (#tag)" autocomplete="off" spellcheck="false"></div>
  <div id="search-results" hidden></div>
  <nav id="file-tree" class="file-tree" aria-label="Notes"></nav>
</aside>
<main class="main" id="main">
  <div class="topbar">
    <button class="icon-btn" id="toggle-left" aria-label="Toggle file list"><svg viewBox="0 0 24 24"><path d="M4 6h16M4 12h16M4 18h16"/></svg></button>
    <div class="topbar-spacer"></div>
    <button class="icon-btn" id="theme-toggle" aria-label="Toggle theme"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 3a9 9 0 0 0 0 18z" fill="currentColor"/></svg></button>
    <button class="icon-btn" id="toggle-right" aria-label="Toggle graph and outline"><svg viewBox="0 0 24 24"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="8" r="2.5"/><circle cx="10" cy="18" r="2.5"/><path d="M8 7l7.6 1M7 8.3l2.4 7.4M16.4 10l-4.6 6"/></svg></button>
  </div>
  <div class="content" id="content">{article}</div>
</main>
<aside class="sidebar sidebar-right" id="sidebar-right">
  <section class="pane graph-pane">
    <div class="pane-header"><span>Graph</span><button class="icon-btn small" id="graph-expand" aria-label="Open global graph"><svg viewBox="0 0 24 24"><path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7"/></svg></button></div>
    <div class="graph-local"><canvas id="graph-local"></canvas></div>
  </section>
  <section class="pane toc-pane">
    <div class="pane-header"><span>On this page</span></div>
    <nav id="toc" class="toc">{toc}</nav>
  </section>
</aside>
</div>
<div class="graph-modal" id="graph-modal" hidden>
  <div class="graph-modal-inner"><canvas id="graph-global"></canvas><button class="icon-btn graph-close" id="graph-close" aria-label="Close graph"><svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6L6 18"/></svg></button></div>
</div>
<div class="tooltip" id="tooltip" hidden></div>
{payload}
<script>window.VWG={config};</script>
<script src="{root}_site/app.js" defer></script>
</body>
</html>
"##,
        site_title = esc(p.site_title),
        mode = json(p.mode),
        theme_link = if p.theme_css { format!("<link rel=\"stylesheet\" href=\"{root}_site/theme.css\">\n") } else { String::new() },
        title = esc(&title),
    )
}

fn lock_html(site_title: &str) -> String {
    format!(
        r#"<div class="lock"><form id="unlock-form" class="lock-form" autocomplete="off">
<svg class="lock-icon" viewBox="0 0 24 24"><rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>
<h1>{}</h1>
<p>This collection is password protected.</p>
<input type="password" id="unlock-password" placeholder="Password" autofocus required>
<label class="remember"><input type="checkbox" id="unlock-remember"> Remember on this device</label>
<button type="submit" id="unlock-submit">Unlock</button>
<p class="unlock-error" id="unlock-error" hidden>Wrong password.</p>
<noscript><p class="unlock-error">JavaScript is required to decrypt this page.</p></noscript>
</form></div>"#,
        esc(site_title)
    )
}

pub fn json(s: &str) -> String {
    // Safe to embed in <script>: '<' is escaped so "</script>" can't appear.
    serde_json::to_string(s).unwrap().replace('<', "\\u003c")
}
