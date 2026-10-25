//! Obsidian-flavoured markdown → HTML, built on comrak's AST.
//!
//! Rendered HTML refers to the slice root through the [`ROOT`] placeholder so the
//! same fragment can be placed at any depth (and reused for the index page).

use crate::vault::{Vault, file_name, scalar_string};
use comrak::adapters::{CodefenceRendererAdapter, HeadingAdapter, HeadingMeta};
use comrak::nodes::{AstNode, NodeValue, Sourcepos};
use comrak::options::Plugins;
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use comrak::{Anchorizer, Arena, Options, format_html_with_plugins, parse_document};
use regex::Regex;
use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::{LazyLock, Mutex};

pub const ROOT: &str = "@@VWG_ROOT@@";
const MAX_EMBED_DEPTH: usize = 4;

static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|[\s(\[])#([\p{L}\p{N}_/\-]*[\p{L}_/\-][\p{L}\p{N}_/\-]*)").unwrap());
static BLOCK_REF_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\^([A-Za-z0-9\-]+)\s*$").unwrap());
static SIZE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)(?:x(\d+))?$").unwrap());

/// What the renderer needs to know about the slice being built.
pub struct SliceCtx<'v> {
    pub vault: &'v Vault,
    /// Note index → output path relative to the slice root ("folder/note.html").
    pub pages: HashMap<usize, String>,
    pub encrypted: bool,
    /// Attachment index → output path relative to the slice root.
    pub media: RefCell<BTreeMap<usize, String>>,
    pub warnings: RefCell<Vec<String>>,
}

impl SliceCtx<'_> {
    fn media_path(&self, attachment: usize) -> String {
        self.media
            .borrow_mut()
            .entry(attachment)
            .or_insert_with(|| {
                let rel = &self.vault.attachments[attachment];
                let hash = hex(&Sha256::digest(rel.as_bytes()));
                if self.encrypted {
                    format!("_media/{}.bin", &hash[..20])
                } else {
                    let name = file_name(rel);
                    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
                    format!("_media/{}-{}.{}", slug(stem), &hash[..8], ext.to_lowercase())
                }
            })
            .clone()
    }

    fn warn(&self, note: usize, msg: String) {
        self.warnings.borrow_mut().push(format!("{}: {msg}", self.vault.notes[note].rel));
    }
}

#[derive(Clone, Debug)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub id: String,
}

#[derive(Default)]
pub struct Rendered {
    pub html: String,
    pub headings: Vec<Heading>,
    /// In-slice notes this note links to or embeds.
    pub links: Vec<usize>,
    /// Names of linked notes that don't exist in the vault.
    pub uncreated: Vec<String>,
    pub text: String,
    pub math: bool,
    pub mermaid: bool,
}

pub struct Renderer {
    syntect: SyntectAdapter,
}

#[derive(Default)]
struct HeadingCollector {
    state: Mutex<(Anchorizer, Vec<Heading>)>,
}

impl HeadingAdapter for HeadingCollector {
    fn enter(&self, out: &mut dyn std::fmt::Write, h: &HeadingMeta, _: Option<Sourcepos>) -> std::fmt::Result {
        let mut state = self.state.lock().unwrap();
        let id = state.0.anchorize(&h.content);
        write!(out, r#"<h{} id="{}" class="heading">"#, h.level, esc(&id))?;
        state.1.push(Heading { level: h.level, text: h.content.trim().to_string(), id });
        Ok(())
    }
    fn exit(&self, out: &mut dyn std::fmt::Write, h: &HeadingMeta) -> std::fmt::Result {
        write!(out, "</h{}>", h.level)
    }
}

struct MermaidRenderer;

impl CodefenceRendererAdapter for MermaidRenderer {
    fn write(&self, out: &mut dyn std::fmt::Write, _: &str, _: &str, code: &str, _: Option<Sourcepos>) -> std::fmt::Result {
        write!(out, r#"<pre class="mermaid">{}</pre>"#, esc(code))
    }
}

fn options() -> Options<'static> {
    let mut o = Options::default();
    let e = &mut o.extension;
    e.strikethrough = true;
    e.table = true;
    e.autolink = true;
    e.tasklist = true;
    e.footnotes = true;
    e.wikilinks_title_after_pipe = true;
    e.highlight = true;
    e.math_dollars = true;
    e.ofm_comments = true;
    o.parse.relaxed_tasklist_matching = true;
    o.render.r#unsafe = true;
    o.render.hardbreaks = true; // Obsidian's default (strict line breaks off)
    o.render.tasklist_classes = true;
    o
}

struct Pass<'r, 'v> {
    ctx: &'r SliceCtx<'v>,
    note: usize,
    /// Notes currently being embedded (cycle guard).
    stack: Vec<usize>,
    out: &'r mut Rendered,
}

impl Renderer {
    pub fn new() -> Self {
        Renderer { syntect: SyntectAdapterBuilder::new().css().build() }
    }

    pub fn render_note(&self, ctx: &SliceCtx, note: usize) -> Rendered {
        let mut out = Rendered::default();
        let body = ctx.vault.notes[note].body.clone();
        let mut pass = Pass { ctx, note, stack: vec![note], out: &mut out };
        let (html, headings) = self.render_md(&mut pass, &body, true);
        out.html = html;
        out.headings = headings;
        out.links.sort_unstable();
        out.links.dedup();
        out
    }

    /// Frontmatter as an Obsidian-style properties table.
    pub fn render_properties(&self, ctx: &SliceCtx, note: usize, hidden: &[String]) -> String {
        let Some(fm) = &ctx.vault.notes[note].frontmatter else { return String::new() };
        let mut rows = String::new();
        let mut scratch = Rendered::default();
        for (k, v) in fm {
            let Some(key) = scalar_string(k) else { continue };
            if hidden.iter().any(|h| h.eq_ignore_ascii_case(&key)) {
                continue;
            }
            let is_tags = matches!(key.to_lowercase().as_str(), "tags" | "tag");
            let value = match v {
                Value::Sequence(items) => items
                    .iter()
                    .map(|i| self.property_value(ctx, note, i, is_tags, &mut scratch))
                    .map(|h| format!(r#"<span class="property-pill">{h}</span>"#))
                    .collect::<String>(),
                Value::String(s) if is_tags => s
                    .split([',', ' '])
                    .filter(|t| !t.is_empty())
                    .map(|t| tag_html(t.trim_start_matches('#')))
                    .collect(),
                other => self.property_value(ctx, note, other, is_tags, &mut scratch),
            };
            write!(rows, r#"<tr><th>{}</th><td>{value}</td></tr>"#, esc(&key)).unwrap();
        }
        if rows.is_empty() {
            return rows;
        }
        format!(
            r#"<details class="properties" open><summary>Properties</summary><table class="properties-table"><tbody>{rows}</tbody></table></details>"#
        )
    }

    fn property_value(&self, ctx: &SliceCtx, note: usize, v: &Value, is_tag: bool, scratch: &mut Rendered) -> String {
        match v {
            Value::Bool(b) => format!(r#"<input type="checkbox" disabled{}>"#, if *b { " checked" } else { "" }),
            Value::String(s) if is_tag => tag_html(s.trim_start_matches('#')),
            Value::String(s) => {
                // Inline markdown so [[links]] and urls work; the paragraph wrapper is dropped.
                let mut pass = Pass { ctx, note, stack: vec![note], out: scratch };
                let (html, _) = self.render_md(&mut pass, s, false);
                let html = html.trim();
                let html = html.strip_prefix("<p>").and_then(|h| h.strip_suffix("</p>")).unwrap_or(html);
                html.to_string()
            }
            Value::Null => String::new(),
            other => scalar_string(other)
                .map(|s| esc(&s))
                .unwrap_or_else(|| esc(serde_yaml::to_string(other).unwrap_or_default().trim())),
        }
    }

    fn render_md(&self, pass: &mut Pass, md: &str, top_level: bool) -> (String, Vec<Heading>) {
        let opts = options();
        let arena = Arena::new();
        let root = parse_document(&arena, md, &opts);

        for node in root.descendants().collect::<Vec<_>>() {
            if matches!(node.data().value, NodeValue::BlockQuote) {
                callout(&arena, node);
            }
        }

        if top_level {
            pass.out.text.push_str(&plain_text(root));
            pass.out.text.push(' ');
        }

        for node in root.descendants().collect::<Vec<_>>() {
            if node.parent().is_none() && !node.same_node(root) {
                continue; // detached by an earlier rewrite
            }
            let value = node.data().value.clone();
            match value {
                NodeValue::Text(t) => text_node(&arena, node, &t),
                NodeValue::WikiLink(wl) if wl.embed => self.embed(&arena, pass, node, &wl.url),
                NodeValue::WikiLink(wl) => wikilink(&arena, pass, node, &wl.url),
                NodeValue::Link(l) => md_link(&arena, pass, node, &l.url),
                NodeValue::Image(l) => md_image(&arena, pass, node, &l.url),
                NodeValue::Math(_) => pass.out.math = true,
                NodeValue::CodeBlock(cb) if cb.info.trim() == "mermaid" => pass.out.mermaid = true,
                _ => {}
            }
        }

        let headings = HeadingCollector::default();
        let mermaid = MermaidRenderer;
        let mut plugins = Plugins::default();
        plugins.render.codefence_syntax_highlighter = Some(&self.syntect);
        plugins.render.heading_adapter = Some(&headings);
        plugins.render.codefence_renderers.insert("mermaid".into(), &mermaid);
        let mut html = String::new();
        format_html_with_plugins(root, &opts, &mut html, &plugins).unwrap();
        let headings = std::mem::take(&mut headings.state.lock().unwrap().1);
        (html, headings)
    }

    fn embed<'a>(&self, arena: &'a Arena<'a>, pass: &mut Pass, node: &'a AstNode<'a>, url: &str) {
        let ctx = pass.ctx;
        let label = node.collect_text();
        let label = if label.trim() == url.trim() { String::new() } else { label };
        let (target, frag) = split_fragment(url);
        let looks_like_file = file_name(target).rsplit_once('.').is_some_and(|(_, ext)| !ext.eq_ignore_ascii_case("md"));

        if looks_like_file {
            if let Some(att) = ctx.vault.resolve_attachment(target, Some(pass.note)) {
                let html = media_html(ctx, att, &label);
                replace(arena, node, html, false);
                return;
            }
        }
        let Some(target_note) = ctx.vault.resolve_note(target, Some(pass.note)) else {
            ctx.warn(pass.note, format!("embed ![[{url}]] does not resolve"));
            replace(arena, node, String::new(), false);
            return;
        };
        let Some(page) = ctx.pages.get(&target_note) else {
            ctx.warn(pass.note, format!("embed ![[{url}]] points outside this slice; omitted"));
            replace(arena, node, String::new(), false);
            return;
        };
        if pass.stack.contains(&target_note) || pass.stack.len() > MAX_EMBED_DEPTH {
            replace(arena, node, String::new(), false);
            return;
        }
        pass.out.links.push(target_note);

        let tn = &ctx.vault.notes[target_note];
        let source = match frag {
            Some(f) => section(&tn.body, f).unwrap_or_default(),
            None => tn.body.clone(),
        };
        pass.stack.push(target_note);
        let saved_note = std::mem::replace(&mut pass.note, target_note);
        let (inner, _) = self.render_md(pass, &source, false);
        pass.note = saved_note;
        pass.stack.pop();

        let title = match frag {
            Some(f) => format!("{} › {}", tn.stem, f.trim_start_matches('^')),
            None => tn.stem.clone(),
        };
        let href = format!("{ROOT}{page}{}", frag.map(fragment_href).unwrap_or_default());
        let html = format!(
            r#"<div class="markdown-embed"><div class="markdown-embed-title"><a class="internal-link" href="{}" data-tooltip="{}">{}</a></div><div class="markdown-embed-content">{inner}</div></div>"#,
            esc(&href),
            esc(&title),
            esc(&title)
        );
        replace(arena, node, html, true);
    }
}

/// `> [!type]± title` blockquotes become callouts; `-`/`+` make them foldable.
fn callout<'a>(arena: &'a Arena<'a>, quote: &'a AstNode<'a>) {
    let Some(para) = quote.first_child().filter(|p| matches!(p.data().value, NodeValue::Paragraph)) else { return };
    let Some(first) = para.first_child() else { return };
    let NodeValue::Text(text) = first.data().value.clone() else { return };
    static HEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[!([^\]]+)\]([+-]?)\s*").unwrap());
    let Some(caps) = HEAD.captures(&text) else { return };
    let kind = caps[1].trim().to_lowercase();
    let fold = caps[2].to_string();
    let rest = text[caps.get(0).unwrap().end()..].to_string();

    // Title inlines run up to the first line break; the remainder is body.
    let title_para = arena.alloc(NodeValue::Paragraph.into());
    if !rest.is_empty() {
        title_para.append(arena.alloc(NodeValue::Text(rest.into()).into()));
    }
    first.detach();
    while let Some(child) = para.first_child() {
        let is_break = matches!(child.data().value, NodeValue::SoftBreak | NodeValue::LineBreak);
        child.detach();
        if is_break {
            break;
        }
        title_para.append(child);
    }
    let has_title = title_para.first_child().is_some();
    if para.first_child().is_none() {
        para.detach();
    }

    let (tag, open_attr) = if fold.is_empty() { ("div", "") } else { ("details", if fold == "+" { " open" } else { "" }) };
    let (title_tag, default_title) = (if fold.is_empty() { "div" } else { "summary" }, title_case(&kind));
    let open = format!(
        r#"<{tag} class="callout" data-callout="{kind}"{open_attr}><{title_tag} class="callout-title"><span class="callout-icon">{icon}</span><span class="callout-title-inner">"#,
        kind = esc(&kind),
        icon = callout_icon(&kind)
    );
    quote.insert_before(html_block(arena, open));
    if has_title {
        quote.insert_before(title_para);
    } else {
        quote.insert_before(html_block(arena, esc(&default_title)));
    }
    quote.insert_before(html_block(arena, format!(r#"</span></{title_tag}><div class="callout-content">"#)));
    while let Some(child) = quote.first_child() {
        child.detach();
        quote.insert_before(child);
    }
    quote.insert_before(html_block(arena, format!("</div></{tag}>")));
    quote.detach();
}

fn text_node<'a>(arena: &'a Arena<'a>, node: &'a AstNode<'a>, text: &str) {
    let in_link = node.ancestors().any(|a| matches!(a.data().value, NodeValue::Link(_) | NodeValue::WikiLink(_)));
    let mut text = text.to_string();

    // Trailing ^block-id marks a block reference target.
    let is_last_in_para =
        node.next_sibling().is_none() && node.parent().is_some_and(|p| matches!(p.data().value, NodeValue::Paragraph));
    if is_last_in_para {
        if let Some(c) = BLOCK_REF_RE.captures(&text) {
            let id = c[1].to_string();
            text.truncate(c.get(0).unwrap().start());
            node.insert_after(html_inline(arena, format!(r#"<span class="block-anchor" id="block-{}"></span>"#, esc(&id))));
        }
    }

    if in_link || !TAG_RE.is_match(&text) {
        node.data_mut().value = NodeValue::Text(text.into());
        return;
    }
    let mut last = 0;
    let mut pieces: Vec<&'a AstNode<'a>> = Vec::new();
    for c in TAG_RE.captures_iter(&text) {
        let tag = c.get(2).unwrap();
        let start = tag.start() - 1; // the '#'
        if start > last {
            pieces.push(arena.alloc(NodeValue::Text(text[last..start].to_string().into()).into()));
        }
        pieces.push(html_inline(arena, tag_html(tag.as_str())));
        last = tag.end();
    }
    if last < text.len() {
        pieces.push(arena.alloc(NodeValue::Text(text[last..].to_string().into()).into()));
    }
    for p in pieces {
        node.insert_before(p);
    }
    node.detach();
}

fn wikilink<'a>(arena: &'a Arena<'a>, pass: &mut Pass, node: &'a AstNode<'a>, url: &str) {
    let ctx = pass.ctx;
    let (target, frag) = split_fragment(url);
    let default_label = node.collect_text().trim() == url.trim();

    if target.is_empty() {
        let frag = frag.unwrap_or_default();
        let open = format!(
            r#"<a class="internal-link" href="{}" data-tooltip="{}">"#,
            esc(&fragment_href(frag)),
            esc(frag.trim_start_matches('^'))
        );
        wrap(arena, node, open, "</a>");
        return;
    }

    if let Some(n) = ctx.vault.resolve_note(target, Some(pass.note)) {
        if let Some(page) = ctx.pages.get(&n) {
            pass.out.links.push(n);
            let stem = &ctx.vault.notes[n].stem;
            let tooltip = match frag {
                Some(f) => format!("{stem} › {}", f.trim_start_matches('^')),
                None => stem.clone(),
            };
            if default_label && frag.is_some() {
                set_label(arena, node, &format!("{target} > {}", frag.unwrap().trim_start_matches('^')));
            }
            let href = format!("{ROOT}{page}{}", frag.map(fragment_href).unwrap_or_default());
            let open = format!(
                r#"<a class="internal-link" href="{}" data-tooltip="{}">"#,
                esc(&href),
                esc(&tooltip)
            );
            wrap(arena, node, open, "</a>");
            return;
        }
    } else if let Some(att) = ctx.vault.resolve_attachment(target, Some(pass.note)) {
        wrap(arena, node, media_link_open(ctx, att), "</a>");
        return;
    }
    unresolved(arena, pass, node, target);
}

fn md_link<'a>(arena: &'a Arena<'a>, pass: &mut Pass, node: &'a AstNode<'a>, url: &str) {
    let ctx = pass.ctx;
    if is_external(url) {
        let open = format!(r#"<a class="external-link" href="{}" target="_blank" rel="noopener noreferrer">"#, esc(url));
        wrap(arena, node, open, "</a>");
        return;
    }
    if url.starts_with('#') {
        return;
    }
    let (target, frag) = split_fragment(url);
    if let Some(n) = ctx.vault.resolve_note(target, Some(pass.note)) {
        if let Some(page) = ctx.pages.get(&n) {
            pass.out.links.push(n);
            let href = format!("{ROOT}{page}{}", frag.map(fragment_href).unwrap_or_default());
            let open = format!(
                r#"<a class="internal-link" href="{}" data-tooltip="{}">"#,
                esc(&href),
                esc(&ctx.vault.notes[n].stem)
            );
            wrap(arena, node, open, "</a>");
            return;
        }
    } else if let Some(att) = ctx.vault.resolve_attachment(target, Some(pass.note)) {
        wrap(arena, node, media_link_open(ctx, att), "</a>");
        return;
    }
    unresolved(arena, pass, node, target);
}

/// A link that doesn't lead to a page of this slice. Notes that exist but aren't
/// shared here keep only their text, so the slice reveals nothing about them.
/// Notes that don't exist anywhere ("uncreated") get a graph node and a link that
/// says so, like Obsidian's unresolved links.
fn unresolved<'a>(arena: &'a Arena<'a>, pass: &mut Pass, node: &'a AstNode<'a>, target: &str) {
    let ctx = pass.ctx;
    if ctx.vault.resolve_note(target, Some(pass.note)).is_some() {
        wrap(arena, node, r#"<span class="internal-link is-unresolved">"#.into(), "</span>");
        return;
    }
    let decoded = crate::vault::percent_decode(target);
    let name = decoded.strip_suffix(".md").unwrap_or(&decoded);
    let name = file_name(name).trim().to_string();
    let open = format!(
        r##"<a class="internal-link is-unresolved is-uncreated" href="#" data-uncreated="{0}" data-tooltip="{0} (not created yet)">"##,
        esc(&name)
    );
    if !pass.out.uncreated.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
        pass.out.uncreated.push(name);
    }
    wrap(arena, node, open, "</a>");
}

fn md_image<'a>(arena: &'a Arena<'a>, pass: &mut Pass, node: &'a AstNode<'a>, url: &str) {
    if is_external(url) || url.starts_with("data:") {
        return;
    }
    let ctx = pass.ctx;
    let Some(att) = ctx.vault.resolve_attachment(url, Some(pass.note)) else {
        ctx.warn(pass.note, format!("image {url} not found"));
        return;
    };
    // Obsidian allows ![alt|300](img.png) for sizing.
    let alt = node.collect_text();
    let html = media_html(ctx, att, &alt);
    replace(arena, node, html, false);
}

fn media_html(ctx: &SliceCtx, att: usize, label: &str) -> String {
    let path = ctx.media_path(att);
    let rel = &ctx.vault.attachments[att];
    let ext = rel.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    let mime = mime_for(&ext);
    let src = if ctx.encrypted {
        format!(r#"data-vwg-src="{ROOT}{}" data-vwg-type="{mime}""#, esc(&path))
    } else {
        format!(r#"src="{ROOT}{}""#, esc(&path))
    };

    // The label is either a size (300 / 300x200) or alt text, optionally "alt|300".
    let (mut alt, mut size) = (label.trim().to_string(), String::new());
    if let Some((a, s)) = label.rsplit_once('|') {
        if SIZE_RE.is_match(s.trim()) {
            alt = a.trim().into();
            size = s.trim().into();
        }
    } else if SIZE_RE.is_match(label.trim()) {
        size = label.trim().into();
        alt = String::new();
    }
    let mut dims = String::new();
    if let Some(c) = SIZE_RE.captures(&size) {
        write!(dims, r#" width="{}""#, &c[1]).unwrap();
        if let Some(h) = c.get(2) {
            write!(dims, r#" height="{}""#, h.as_str()).unwrap();
        }
    }

    match mime.split('/').next().unwrap() {
        "image" => format!(r#"<img {src} alt="{}"{dims} loading="lazy">"#, esc(&alt)),
        "audio" => format!(r#"<audio controls {src}></audio>"#),
        "video" => format!(r#"<video controls {src}{dims}></video>"#),
        _ if ext == "pdf" => format!(r#"<iframe class="pdf-embed" {src}></iframe>"#),
        _ => format!(r#"{}{}</a>"#, media_link_open(ctx, att), esc(file_name(rel))),
    }
}

fn media_link_open(ctx: &SliceCtx, att: usize) -> String {
    let path = ctx.media_path(att);
    let name = file_name(&ctx.vault.attachments[att]);
    if ctx.encrypted {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
        format!(
            r##"<a class="internal-link attachment-link" href="#" data-vwg-href="{ROOT}{}" data-vwg-type="{}" data-vwg-name="{}">"##,
            esc(&path),
            mime_for(&ext),
            esc(name)
        )
    } else {
        format!(r#"<a class="internal-link attachment-link" href="{ROOT}{}" data-tooltip="{}">"#, esc(&path), esc(name))
    }
}

/// Markdown of a heading's section, or of the block carrying `^id`.
fn section(body: &str, frag: &str) -> Option<String> {
    if let Some(id) = frag.strip_prefix('^') {
        let marker = format!("^{id}");
        let blocks: Vec<&str> = body.split("\n\n").collect();
        let block = blocks.iter().find(|b| b.trim_end().ends_with(&marker))?;
        return Some(block.trim_end().trim_end_matches(&marker).to_string());
    }
    let want = Anchorizer::new().anchorize(frag);
    let mut out: Option<(usize, String)> = None;
    let mut in_fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        let level = if in_fence { 0 } else { line.chars().take_while(|&c| c == '#').count() };
        let is_heading = (1..=6).contains(&level) && line[level..].starts_with(' ');
        match &mut out {
            Some((lvl, buf)) => {
                if is_heading && level <= *lvl {
                    break;
                }
                buf.push_str(line);
                buf.push('\n');
            }
            None if is_heading && Anchorizer::new().anchorize(line[level..].trim()) == want => {
                out = Some((level, format!("{line}\n")));
            }
            None => {}
        }
    }
    out.map(|(_, s)| s)
}

fn split_fragment(url: &str) -> (&str, Option<&str>) {
    match url.split_once('#') {
        Some((t, f)) => (t.trim(), Some(f.trim())),
        None => (url.trim(), None),
    }
}

fn fragment_href(frag: &str) -> String {
    match frag.strip_prefix('^') {
        Some(id) => format!("#block-{id}"),
        None => format!("#{}", Anchorizer::new().anchorize(frag)),
    }
}

fn is_external(url: &str) -> bool {
    let scheme = url.split_once(':').map(|(s, _)| s);
    scheme.is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)))
}

fn tag_html(tag: &str) -> String {
    format!(
        r#"<a class="tag" href="{ROOT}index.html?q=%23{}" data-tag="{}">#{}</a>"#,
        esc(tag),
        esc(&tag.to_lowercase()),
        esc(tag)
    )
}

fn plain_text<'a>(root: &'a AstNode<'a>) -> String {
    let mut s = String::new();
    for n in root.descendants() {
        match &n.data().value {
            NodeValue::Text(t) => s.push_str(t),
            NodeValue::Code(c) => s.push_str(&c.literal),
            NodeValue::WikiLink(_) | NodeValue::SoftBreak | NodeValue::LineBreak => s.push(' '),
            NodeValue::Paragraph | NodeValue::Heading(_) | NodeValue::Item(_) | NodeValue::TableCell => s.push(' '),
            _ => {}
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn html_inline<'a>(arena: &'a Arena<'a>, html: String) -> &'a AstNode<'a> {
    arena.alloc(NodeValue::HtmlInline(html).into())
}

fn html_block<'a>(arena: &'a Arena<'a>, html: String) -> &'a AstNode<'a> {
    arena.alloc(NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock { block_type: 6, literal: format!("{html}\n") }).into())
}

/// Replaces `node` with its children wrapped in raw `open`/`close` HTML.
fn wrap<'a>(arena: &'a Arena<'a>, node: &'a AstNode<'a>, open: String, close: &str) {
    node.insert_before(html_inline(arena, open));
    while let Some(child) = node.first_child() {
        child.detach();
        node.insert_before(child);
    }
    node.insert_before(html_inline(arena, close.to_string()));
    node.detach();
}

fn set_label<'a>(arena: &'a Arena<'a>, node: &'a AstNode<'a>, label: &str) {
    while let Some(c) = node.first_child() {
        c.detach();
    }
    node.append(arena.alloc(NodeValue::Text(label.to_string().into()).into()));
}

/// Replaces `node` with raw HTML. Block-level content standing alone in a
/// paragraph replaces the paragraph so it doesn't end up inside a `<p>`.
fn replace<'a>(arena: &'a Arena<'a>, node: &'a AstNode<'a>, html: String, block: bool) {
    if block {
        if let Some(p) = node.parent().filter(|p| matches!(p.data().value, NodeValue::Paragraph)) {
            let alone = p.children().all(|c| {
                c.same_node(node)
                    || matches!(&c.data().value, NodeValue::SoftBreak | NodeValue::LineBreak)
                    || matches!(&c.data().value, NodeValue::Text(t) if t.trim().is_empty())
            });
            if alone {
                p.insert_before(html_block(arena, html));
                p.detach();
                return;
            }
        }
    }
    node.insert_before(html_inline(arena, html));
    node.detach();
}

fn callout_icon(kind: &str) -> &'static str {
    let body = match kind {
        "abstract" | "summary" | "tldr" => {
            r#"<rect width="8" height="4" x="8" y="2" rx="1" ry="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/><path d="M12 11h4"/><path d="M12 16h4"/><path d="M8 11h.01"/><path d="M8 16h.01"/>"#
        }
        "info" => r#"<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>"#,
        "todo" => r#"<circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/>"#,
        "tip" | "hint" | "important" => {
            r#"<path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5z"/>"#
        }
        "success" | "check" | "done" => r#"<path d="M20 6 9 17l-5-5"/>"#,
        "question" | "help" | "faq" => {
            r#"<circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><path d="M12 17h.01"/>"#
        }
        "warning" | "caution" | "attention" => {
            r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#
        }
        "failure" | "fail" | "missing" => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
        "danger" | "error" => r#"<path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/>"#,
        "bug" => {
            r#"<path d="m8 2 1.88 1.88"/><path d="M14.12 3.88 16 2"/><path d="M9 7.13v-1a3.003 3.003 0 1 1 6 0v1"/><path d="M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v3c0 3.3-2.7 6-6 6"/><path d="M12 20v-9"/><path d="M6.53 9C4.6 8.8 3 7.1 3 5"/><path d="M6 13H2"/><path d="M3 21c0-2.1 1.7-3.9 3.8-4"/><path d="M20.97 5c0 2.1-1.6 3.8-3.5 4"/><path d="M22 13h-4"/><path d="M17.2 17c2.1.1 3.8 1.9 3.8 4"/>"#
        }
        "example" => {
            r#"<path d="M8 6h13"/><path d="M8 12h13"/><path d="M8 18h13"/><path d="M3 6h.01"/><path d="M3 12h.01"/><path d="M3 18h.01"/>"#
        }
        "quote" | "cite" => {
            r#"<path d="M3 21c3 0 7-1 7-8V5c0-1.25-.756-2.017-2-2H4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2 1 0 1 0 1 1v1c0 1-1 2-2 2s-1 .008-1 1.031V20c0 1 0 1 1 1z"/><path d="M15 21c3 0 7-1 7-8V5c0-1.25-.757-2.017-2-2h-4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2h.75c0 2.25.25 4-2.75 4v3c0 1 0 1 1 1z"/>"#
        }
        _ => r#"<path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/><path d="m15 5 4 4"/>"#,
    };
    // Leak the formatted string once per kind; there are only a handful.
    static CACHE: LazyLock<Mutex<HashMap<&'static str, &'static str>>> = LazyLock::new(Default::default);
    CACHE.lock().unwrap().entry(body).or_insert_with(|| {
        Box::leak(
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="svg-icon">{body}</svg>"#
            )
            .into_boxed_str(),
        )
    })
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

pub fn mime_for(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "untitled".into() } else { out }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
