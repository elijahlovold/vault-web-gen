# vault-web-gen

Exports slices of an Obsidian vault as static, self-contained websites. Check out a demo here:

<https://elovold.xyz/vault>

## Usage

```sh
vault-web-gen -v ~/vault init     # writes a sample ~/vault/vault-web.toml
vault-web-gen -v ~/vault list     # each slice's link (from base_url) and what it exports / skips
vault-web-gen -v ~/vault build    # renders into site.output (replaced on every build)
vault-web-gen -v ~/vault serve    # preview; prints a localhost link for every slice
```

## Layout

```
<output>/            public slice  → /vault/
<output>/<uuid>/     each [[slice]] → /vault/<uuid>/
```

Every slice has its own `_site/` (css/js/data) and `_media/`, and links only to its own pages.
A link to a note outside the slice is plain text. Hovering it shows "Name (not shared)",
unless `show_unshared_names = false`. A link to a note that doesn't exist anywhere shows
"not created yet" and gets a hollow node in the graph.
The public slice never references the slices.

## Caddy

Pages use relative links, so a slice must be opened as a directory URL *with* the trailing
slash (`/vault/<uuid>/`). Caddy's `file_server` redirects `/vault/<uuid>` to that by default.
Each index page also fixes this itself with a one-line script, in case a `handle_path`/`uri strip_prefix`
setup breaks the redirect. A minimal site block:

```
example.com {
    handle /vault/* {
        root * /srv            # /srv/vault/ is the copied output folder
        file_server            # no `browse`
    }
}
```

## Config (`vault-web.toml` in the vault root)

```toml
[site]
output = "../site/vault"        # relative to the vault root
base_url = "https://example.com/vault/"   # optional, only for printing share links
require_tag = "shared"          # frontmatter tag required on EVERY exported note
exclude = ["templates/**"]      # never exported (dot-folders are always skipped)
hide_properties = ["cssclasses"]
cdn = true                      # KaTeX / Mermaid from jsDelivr, only on pages that use them
show_unshared_names = true      # hover on a link to an unshared note shows "Name (not shared)"
theme = "auto"                  # "auto" (active Obsidian theme), "none", or a theme name
# snippets = ["my-snippet"]     # default: the snippets enabled in Obsidian
# color_mode = "system"         # initial light/dark: "system" | "dark" | "light"; default: Obsidian's

[public]                        # optional; no password allowed
title = "Notes"
home = "Home"                   # optional; otherwise index.html lists all notes
include = ["reflections/"]      # globs; "folder" or "folder/" matches everything below it
tags = ["public"]               # notes with these tags (or nested children: public/x)
notes = ["Some Note"]           # individual notes, wikilink syntax

[[slice]]
name = "alice"                  # local label, never published
id = "…"                        # written back automatically on first build — keep it
title = "Notes for Alice"
password = "…"                  # or password_env = "ALICE_PW"; omit for no password
include = ["projects/foo/**"]
exclude = []
tags = ["share/alice"]
```

A note is in a slice if it matches `include`, `tags` or `notes`, isn't excluded, **and** has
`require_tag` in its frontmatter `tags`. Matching notes without the tag are listed as skipped.

## Themes

By default each slice gets your Obsidian look: the active theme
(`.obsidian/themes/<name>/theme.css`), the enabled snippets (`.obsidian/snippets/*.css`),
and the accent color, fonts, base font size and light/dark mode from `.obsidian/appearance.json`.
They're bundled into `_site/theme.css`. The site's CSS uses Obsidian's variable names and
classes (`.theme-dark`, `--background-primary`, `.markdown-rendered`, `.callout`…), so themes
apply as they do in the app. Settings made through the Style Settings plugin are not applied yet,
so a theme shows its defaults (e.g. Catppuccin: Mocha in dark mode, Latte in light).
Fonts only render if the visitor has them installed.

## Passwords

Fully static: page content, the site index (tree/search/graph) and media of a protected slice are
AES-256-GCM encrypted at build time (PBKDF2-SHA256, 600k iterations) and decrypted in the browser
with WebCrypto, which requires https (or localhost). The derived key is kept in sessionStorage
(or localStorage with "Remember on this device"). Readable page file names and folder structure
are not encrypted, so leave Caddy's `browse` off. Pages of slices carry `noindex` and `no-referrer`.

## Supported markdown

Frontmatter → properties table, wikilinks (`[[note]]`, `[[note|alias]]`, `[[note#heading]]`,
`[[note#^block]]`) with a hover tooltip showing the real note name, markdown links to notes,
embeds (`![[img.png|300]]`, audio/video/pdf, `![[note]]`, `![[note#section]]`), callouts (any type,
foldable `-`/`+`, nested), tables, task lists, footnotes, `==highlight==`, `%%comments%%`,
inline `#tags` (click to search), syntax highlighting, math, mermaid, backlinks, outline, local
and global graph, full-text search, light/dark theme.
