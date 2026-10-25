# vault-web-gen

Exports slices of an Obsidian vault as static, self-contained websites. Copy the output
directory to your webserver's `/vault/` and you're done.

```
vault-web-gen -v ~/vault init     # writes a sample ~/vault/vault-web.toml
vault-web-gen -v ~/vault list     # what each slice exports / skips
vault-web-gen -v ~/vault build    # renders into site.output (replaced on every build)
vault-web-gen -v ~/vault serve    # preview at http://127.0.0.1:8080/vault/
```

## Layout

```
<output>/            public slice  → /vault/
<output>/<uuid>/     each [[slice]] → /vault/<uuid>/
```

Every slice has its own `_site/` (css/js/data) and `_media/`, and links only to its own pages.
A link to a note outside the slice is shown as plain text, without the target's name.
The public slice never references the slices.

## Config (`vault-web.toml` in the vault root)

```toml
[site]
output = "../site/vault"        # relative to the vault root
base_url = "https://example.com/vault/"   # optional, only for printing share links
require_tag = "shared"          # frontmatter tag required on EVERY exported note
exclude = ["templates/**"]      # never exported (dot-folders are always skipped)
hide_properties = ["cssclasses"]
cdn = true                      # KaTeX / Mermaid from jsDelivr, only on pages that use them

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
