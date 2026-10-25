// vault-web-gen client: unlock, file tree, search, graph, tooltips.
(() => {
  "use strict";
  const C = window.VWG;
  const $ = (s, el = document) => el.querySelector(s);
  const $$ = (s, el = document) => Array.from(el.querySelectorAll(s));
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const store = {
    get: (k, s = localStorage) => { try { return s.getItem(k); } catch { return null; } },
    set: (k, v, s = localStorage) => { try { s.setItem(k, v); } catch {} },
    del: (k) => { try { localStorage.removeItem(k); sessionStorage.removeItem(k); } catch {} },
  };
  const narrow = matchMedia("(max-width: 1000px)");

  let key = null; // AES-GCM CryptoKey (encrypted slices only)
  let data = null; // { notes: [{t, p, f, g}], links: [[a, b]], text: [] }
  const graphs = [];

  // ---------------------------------------------------------------- theme
  function applyTheme(t) {
    document.documentElement.dataset.theme = t;
    $("#hl-light").disabled = t === "dark";
    $("#hl-dark").disabled = t !== "dark";
    graphs.forEach((g) => g.readColors());
  }
  applyTheme(document.documentElement.dataset.theme || "light");
  $("#theme-toggle").addEventListener("click", () => {
    const t = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    store.set("vwg-theme", t);
    applyTheme(t);
  });

  // ---------------------------------------------------------------- sidebars
  for (const side of ["left", "right"]) {
    if (store.get("vwg-hide-" + side) === "1") document.body.classList.add("hide-" + side);
    $("#toggle-" + side).addEventListener("click", (e) => {
      e.stopPropagation();
      if (narrow.matches) {
        document.body.classList.toggle("show-" + side);
        document.body.classList.remove("show-" + (side === "left" ? "right" : "left"));
      } else {
        const hidden = document.body.classList.toggle("hide-" + side);
        store.set("vwg-hide-" + side, hidden ? "1" : "0");
      }
    });
  }
  $("#main").addEventListener("click", () => document.body.classList.remove("show-left", "show-right"));

  // ---------------------------------------------------------------- crypto
  const b64 = (s) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
  const toB64 = (u8) => btoa(String.fromCharCode(...u8));
  const keyName = () => "vwg-key:" + C.encrypted.slice;

  async function deriveRawKey(password) {
    const base = await crypto.subtle.importKey("raw", new TextEncoder().encode(password), "PBKDF2", false, ["deriveBits"]);
    const bits = await crypto.subtle.deriveBits(
      { name: "PBKDF2", hash: "SHA-256", salt: b64(C.encrypted.salt), iterations: C.encrypted.iterations },
      base,
      256,
    );
    return new Uint8Array(bits);
  }
  const importKey = (raw) => crypto.subtle.importKey("raw", raw, "AES-GCM", false, ["decrypt"]);
  async function decrypt(bytes) {
    const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: bytes.slice(0, 12) }, key, bytes.slice(12));
    return new Uint8Array(plain);
  }
  async function fetchBytes(url) {
    const res = await fetch(url);
    if (!res.ok) throw new Error(res.status + " " + url);
    return new Uint8Array(await res.arrayBuffer());
  }

  async function openPayload() {
    const payload = b64($("#vwg-payload").textContent.trim());
    const page = JSON.parse(new TextDecoder().decode(await decrypt(payload)));
    document.title = page.title + " · " + document.title;
    $("#content").innerHTML = page.article;
    $("#toc").innerHTML = page.toc;
    decryptMedia();
  }

  function unlock() {
    return new Promise((resolve) => {
      const form = $("#unlock-form");
      const error = $("#unlock-error");
      if (!window.crypto || !crypto.subtle) {
        error.textContent = "Decryption needs a secure (https) connection.";
        error.hidden = false;
        return;
      }
      const saved = store.get(keyName(), sessionStorage) || store.get(keyName());
      const attempt = async (raw) => {
        key = await importKey(raw);
        await openPayload();
        return raw;
      };
      if (saved) {
        form.hidden = true;
        attempt(b64(saved)).then(resolve, () => {
          store.del(keyName());
          form.hidden = false;
        });
      }
      form.addEventListener("submit", async (e) => {
        e.preventDefault();
        const button = $("#unlock-submit");
        button.disabled = true;
        error.hidden = true;
        // Read the form before decrypting: a successful attempt replaces it.
        const remember = $("#unlock-remember").checked;
        try {
          const raw = await attempt(await deriveRawKey($("#unlock-password").value));
          store.set(keyName(), toB64(raw), remember ? localStorage : sessionStorage);
          resolve();
        } catch {
          key = null;
          error.textContent = "Wrong password.";
          error.hidden = false;
          button.disabled = false;
        }
      });
    });
  }

  function decryptMedia() {
    for (const el of $$("[data-vwg-src]")) {
      fetchBytes(el.dataset.vwgSrc)
        .then(decrypt)
        .then((bytes) => (el.src = URL.createObjectURL(new Blob([bytes], { type: el.dataset.vwgType }))))
        .catch((e) => console.error("media", e));
    }
  }
  document.addEventListener("click", async (e) => {
    const a = e.target.closest("[data-vwg-href]");
    if (!a) return;
    e.preventDefault();
    const bytes = await decrypt(await fetchBytes(a.dataset.vwgHref));
    const url = URL.createObjectURL(new Blob([bytes], { type: a.dataset.vwgType }));
    const link = Object.assign(document.createElement("a"), { href: url, target: "_blank", rel: "noopener" });
    if (a.dataset.vwgType === "application/octet-stream") link.download = a.dataset.vwgName;
    link.click();
  });

  async function loadData() {
    if (C.encrypted) {
      const bytes = await decrypt(await fetchBytes(C.root + "_site/data.bin"));
      return JSON.parse(new TextDecoder().decode(bytes));
    }
    const res = await fetch(C.root + "_site/data.json");
    return res.json();
  }

  // ---------------------------------------------------------------- content
  function enhanceContent() {
    for (const pre of $$("#content pre:not(.mermaid)")) {
      const btn = Object.assign(document.createElement("button"), { className: "copy-code", textContent: "Copy" });
      btn.addEventListener("click", () => {
        navigator.clipboard?.writeText(pre.querySelector("code")?.innerText ?? pre.innerText);
        btn.textContent = "Copied";
        setTimeout(() => (btn.textContent = "Copy"), 1200);
      });
      pre.appendChild(btn);
    }
    if (location.hash) {
      const target = document.getElementById(decodeURIComponent(location.hash.slice(1)));
      target?.scrollIntoView();
    }
    if (C.cdn && C.math) loadMath();
    if (C.cdn && C.mermaid) loadMermaid();
    setupTocSpy();
  }

  function loadScript(src) {
    return new Promise((ok, fail) => {
      const s = Object.assign(document.createElement("script"), { src, onload: ok, onerror: fail, crossOrigin: "anonymous" });
      document.head.appendChild(s);
    });
  }
  async function loadMath() {
    const base = "https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/";
    document.head.appendChild(Object.assign(document.createElement("link"), { rel: "stylesheet", href: base + "katex.min.css" }));
    await loadScript(base + "katex.min.js");
    for (const el of $$("[data-math-style]")) {
      window.katex.render(el.textContent, el, { displayMode: el.dataset.mathStyle === "display", throwOnError: false });
    }
  }
  async function loadMermaid() {
    await loadScript("https://cdn.jsdelivr.net/npm/mermaid@10.9.1/dist/mermaid.min.js");
    const dark = document.documentElement.dataset.theme === "dark";
    window.mermaid.initialize({ startOnLoad: false, theme: dark ? "dark" : "default" });
    window.mermaid.run({ querySelector: "pre.mermaid" });
  }

  function setupTocSpy() {
    const items = $$("#toc .toc-item");
    if (!items.length) return;
    const headings = items.map((a) => document.getElementById(decodeURIComponent(a.hash.slice(1)))).filter(Boolean);
    const update = () => {
      let active = 0;
      headings.forEach((h, i) => { if (h.getBoundingClientRect().top < 90) active = i; });
      items.forEach((a, i) => a.classList.toggle("is-active", i === active));
    };
    $("#main").addEventListener("scroll", update, { passive: true });
    update();
  }

  // ---------------------------------------------------------------- uncreated notes
  let toastTimer;
  function notCreated(name) {
    let toast = $("#toast");
    if (!toast) {
      toast = Object.assign(document.createElement("div"), { id: "toast", className: "toast" });
      toast.setAttribute("role", "status");
      document.body.appendChild(toast);
    }
    toast.innerHTML = `<strong>${esc(name)}</strong> hasn't been created yet.`;
    toast.classList.add("is-visible");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toast.classList.remove("is-visible"), 2600);
  }
  document.addEventListener("click", (e) => {
    const a = e.target.closest("[data-uncreated]");
    if (!a) return;
    e.preventDefault();
    notCreated(a.dataset.uncreated);
  });

  // ---------------------------------------------------------------- tooltips
  const tooltip = $("#tooltip");
  document.addEventListener("mouseover", (e) => {
    const el = e.target.closest("[data-tooltip]");
    if (!el) return;
    const text = el.dataset.tooltip;
    if (!text || text.trim().toLowerCase() === el.textContent.trim().toLowerCase()) return;
    tooltip.textContent = text;
    tooltip.hidden = false;
    const r = el.getClientRects()[0] || el.getBoundingClientRect();
    const t = tooltip.getBoundingClientRect();
    const left = Math.min(Math.max(8, r.left + r.width / 2 - t.width / 2), innerWidth - t.width - 8);
    const top = r.top - t.height - 6 < 4 ? r.bottom + 6 : r.top - t.height - 6;
    tooltip.style.left = left + "px";
    tooltip.style.top = top + "px";
  });
  document.addEventListener("mouseout", (e) => {
    if (e.target.closest("[data-tooltip]")) tooltip.hidden = true;
  });

  // ---------------------------------------------------------------- file tree
  function buildTree() {
    const tree = { folders: new Map(), files: [] };
    data.notes.forEach((n, i) => {
      if (n.u) return; // uncreated: graph only
      let node = tree;
      for (const part of n.f ? n.f.split("/") : []) {
        if (!node.folders.has(part)) node.folders.set(part, { folders: new Map(), files: [] });
        node = node.folders.get(part);
      }
      node.files.push(i);
    });
    const contains = (node, path) =>
      node.files.some((i) => data.notes[i].p === path) || [...node.folders.values()].some((f) => contains(f, path));
    const render = (node) => {
      let html = "";
      for (const [name, sub] of [...node.folders].sort((a, b) => a[0].localeCompare(b[0]))) {
        const open = (C.note && contains(sub, C.note)) || data.notes.length <= 40 ? " open" : "";
        html += `<details${open}><summary class="tree-folder">${esc(name)}</summary><div class="tree-children">${render(sub)}</div></details>`;
      }
      for (const i of [...node.files].sort((a, b) => data.notes[a].t.localeCompare(data.notes[b].t))) {
        const n = data.notes[i];
        const active = n.p === C.note ? " is-active" : "";
        html += `<a class="tree-file${active}" href="${esc(C.root + n.p)}">${esc(n.t)}</a>`;
      }
      return html;
    };
    $("#file-tree").innerHTML = render(tree);
    $("#file-tree .is-active")?.scrollIntoView({ block: "center" });
  }

  // ---------------------------------------------------------------- search
  function setupSearch() {
    const input = $("#search-input");
    const results = $("#search-results");
    const titles = data.notes.map((n) => n.t.toLowerCase());
    const texts = data.text.map((t) => t.toLowerCase());
    let timer;

    const run = () => {
      const q = input.value.trim().toLowerCase();
      $("#file-tree").hidden = !!q;
      results.hidden = !q;
      if (!q) return;
      const terms = q.split(/\s+/);
      const words = terms.filter((t) => !t.startsWith("#"));
      const hits = [];
      data.notes.forEach((n, i) => {
        if (n.u) return;
        let score = 0;
        for (const term of terms) {
          if (term.startsWith("#")) {
            const tag = term.slice(1);
            if (!n.g.some((g) => g === tag || g.startsWith(tag + "/") || (tag && g.startsWith(tag)))) return;
            score += 3;
          } else if (titles[i].includes(term)) score += 10;
          else if (texts[i].includes(term)) score += 1;
          else return;
        }
        if (words[0] && titles[i].startsWith(words[0])) score += 5;
        hits.push([score, i]);
      });
      hits.sort((a, b) => b[0] - a[0] || data.notes[a[1]].t.localeCompare(data.notes[b[1]].t));
      results.innerHTML = hits.length
        ? hits.slice(0, 60).map(([, i]) => {
            const n = data.notes[i];
            return `<a class="search-result" href="${esc(C.root + n.p)}"><div class="search-result-title">${mark(n.t, words)}</div>${snippet(i, words)}</a>`;
          }).join("")
        : `<div class="search-empty">No results</div>`;
    };

    const mark = (text, words) => {
      let html = esc(text);
      for (const w of words) {
        if (!w) continue;
        const re = new RegExp(esc(w).replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi");
        html = html.replace(re, (m) => `<mark>${m}</mark>`);
      }
      return html;
    };
    const snippet = (i, words) => {
      const w = words.find((w) => texts[i].includes(w));
      if (!w) return "";
      const at = texts[i].indexOf(w);
      const start = Math.max(0, at - 60);
      const text = (start ? "…" : "") + data.text[i].slice(start, at + w.length + 90) + "…";
      return `<div class="search-result-snippet">${mark(text, words)}</div>`;
    };

    input.addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(run, 60); });
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") $(".search-result", results)?.click();
      if (e.key === "Escape") { input.value = ""; run(); input.blur(); }
    });
    document.addEventListener("keydown", (e) => {
      if ((e.key === "k" && (e.ctrlKey || e.metaKey)) || (e.key === "/" && !/INPUT|TEXTAREA/.test(document.activeElement.tagName))) {
        e.preventDefault();
        if (narrow.matches) document.body.classList.add("show-left");
        else document.body.classList.remove("hide-left");
        input.focus();
      }
    });
    document.addEventListener("click", (e) => {
      const tag = e.target.closest("a.tag");
      if (!tag) return;
      e.preventDefault();
      e.stopPropagation();
      input.value = "#" + tag.dataset.tag;
      if (narrow.matches) document.body.classList.add("show-left");
      else document.body.classList.remove("hide-left");
      run();
    });

    const q = new URLSearchParams(location.search).get("q");
    if (q) {
      input.value = q;
      if (narrow.matches) document.body.classList.add("show-left");
      run();
    }
  }

  // ---------------------------------------------------------------- graph
  class Graph {
    constructor(canvas, nodes, links, current, labels) {
      this.canvas = canvas;
      this.ctx = canvas.getContext("2d");
      this.nodes = nodes.map((n, i) => {
        const a = i * 2.399963; // golden-angle spiral: stable initial layout
        const r = 12 * Math.sqrt(i + 1);
        return { ...n, x: Math.cos(a) * r, y: Math.sin(a) * r, vx: 0, vy: 0, deg: 0, adj: new Set() };
      });
      this.links = links;
      for (const [a, b] of links) {
        this.nodes[a].deg++; this.nodes[b].deg++;
        this.nodes[a].adj.add(b); this.nodes[b].adj.add(a);
      }
      this.current = current;
      this.labels = labels;
      this.scale = 1; this.tx = 0; this.ty = 0;
      this.alpha = 1;
      this.hover = -1;
      this.readColors();
      for (let i = 0; i < 300 && this.alpha > 0.02 && this.nodes.length < 1500; i++) this.step();
      this.resize();
      this.fit();
      this.bind();
      new ResizeObserver(() => { this.resize(); this.draw(); }).observe(canvas);
      this.kick();
    }
    readColors() {
      const s = getComputedStyle(document.documentElement);
      const v = (n) => s.getPropertyValue(n).trim();
      this.colors = { node: v("--graph-node"), line: v("--graph-line"), label: v("--graph-label"), accent: v("--accent"), bg: v("--bg") };
      this.draw?.();
    }
    step() {
      const n = this.nodes, len = n.length, a = this.alpha;
      for (let i = 0; i < len; i++) {
        for (let j = i + 1; j < len; j++) {
          let dx = n[j].x - n[i].x, dy = n[j].y - n[i].y;
          let d2 = dx * dx + dy * dy;
          if (d2 < 1) { dx = Math.random() - 0.5; dy = Math.random() - 0.5; d2 = 1; }
          if (d2 > 160000) continue;
          const f = (900 * a) / d2;
          n[i].vx -= dx * f; n[i].vy -= dy * f;
          n[j].vx += dx * f; n[j].vy += dy * f;
        }
      }
      for (const [s, t] of this.links) {
        const A = n[s], B = n[t];
        const dx = B.x - A.x, dy = B.y - A.y;
        const d = Math.sqrt(dx * dx + dy * dy) || 1;
        const f = ((d - 55) / d) * 0.06 * a;
        A.vx += dx * f; A.vy += dy * f;
        B.vx -= dx * f; B.vy -= dy * f;
      }
      for (const p of n) {
        // Isolated notes have no springs holding them, so they get a stronger pull.
        const g = (p.deg ? 0.012 : 0.04) * a;
        p.vx -= p.x * g; p.vy -= p.y * g;
        if (p.fixed) { p.vx = p.vy = 0; continue; }
        p.vx *= 0.6; p.vy *= 0.6;
        p.x += p.vx; p.y += p.vy;
      }
      this.alpha *= 0.985;
    }
    kick(alpha) {
      if (alpha) this.alpha = Math.max(this.alpha, alpha);
      if (this.running) return;
      this.running = true;
      const tick = () => {
        if (this.alpha > 0.01) this.step();
        this.draw();
        if (this.alpha > 0.01 || this.drag) requestAnimationFrame(tick);
        else this.running = false;
      };
      requestAnimationFrame(tick);
    }
    resize() {
      const dpr = devicePixelRatio || 1;
      this.w = this.canvas.clientWidth; this.h = this.canvas.clientHeight;
      this.canvas.width = this.w * dpr; this.canvas.height = this.h * dpr;
      this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }
    fit() {
      if (!this.nodes.length || !this.w) return;
      const xs = this.nodes.map((p) => p.x), ys = this.nodes.map((p) => p.y);
      const minX = Math.min(...xs), maxX = Math.max(...xs), minY = Math.min(...ys), maxY = Math.max(...ys);
      // Leave room for labels, which hang below and extend sideways from each node.
      const padX = this.labels ? Math.min(90, this.w / 4) : 30, padY = 36;
      this.scale = Math.min(2.2, (this.w - padX * 2) / Math.max(1, maxX - minX), (this.h - padY * 2) / Math.max(1, maxY - minY));
      this.tx = -((minX + maxX) / 2) * this.scale;
      this.ty = -((minY + maxY) / 2) * this.scale;
    }
    toScreen(p) { return [p.x * this.scale + this.w / 2 + this.tx, p.y * this.scale + this.h / 2 + this.ty]; }
    toWorld(x, y) { return [(x - this.w / 2 - this.tx) / this.scale, (y - this.h / 2 - this.ty) / this.scale]; }
    radius(p) { return 4 + Math.sqrt(p.deg) * 1.6; }
    nodeScale() { return Math.min(1.4, Math.max(this.nodes.length <= 30 ? 1 : 0.6, this.scale)); }
    nodeAt(x, y) {
      let best = -1, bestD = Infinity;
      this.nodes.forEach((p, i) => {
        const [sx, sy] = this.toScreen(p);
        const d = Math.hypot(sx - x, sy - y);
        if (d < Math.max(8, this.radius(p) * this.scale + 3) && d < bestD) { best = i; bestD = d; }
      });
      return best;
    }
    draw() {
      const { ctx, colors, nodes } = this;
      if (!this.w) return;
      ctx.clearRect(0, 0, this.w, this.h);
      const focus = this.hover >= 0 ? this.hover : -1;
      const near = (i) => focus < 0 || i === focus || nodes[focus].adj.has(i);
      ctx.lineWidth = 1;
      for (const [a, b] of this.links) {
        const lit = focus >= 0 && (a === focus || b === focus);
        ctx.strokeStyle = lit ? colors.accent : colors.line;
        ctx.globalAlpha = focus >= 0 && !lit ? 0.35 : 1;
        const [x1, y1] = this.toScreen(nodes[a]), [x2, y2] = this.toScreen(nodes[b]);
        ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke();
      }
      nodes.forEach((p, i) => {
        const [x, y] = this.toScreen(p);
        ctx.globalAlpha = near(i) ? 1 : 0.3;
        const r = this.radius(p) * this.nodeScale();
        ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2);
        if (p.u) {
          // Uncreated note: hollow and faded, like Obsidian's unresolved nodes.
          ctx.globalAlpha *= 0.6;
          ctx.fillStyle = colors.bg; ctx.fill();
          ctx.lineWidth = 1.5;
          ctx.strokeStyle = i === focus ? colors.accent : colors.node; ctx.stroke();
          ctx.lineWidth = 1;
        } else {
          ctx.fillStyle = i === this.current || i === focus ? colors.accent : colors.node;
          ctx.fill();
        }
      });
      ctx.font = "11px " + getComputedStyle(document.body).fontFamily;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      nodes.forEach((p, i) => {
        const show = this.labels || this.scale > 1.1 || i === focus || i === this.current || (focus >= 0 && nodes[focus].adj.has(i));
        if (!show) return;
        const [x, y] = this.toScreen(p);
        ctx.globalAlpha = near(i) ? 1 : 0.25;
        ctx.fillStyle = colors.label;
        const label = p.t.length > 28 ? p.t.slice(0, 27) + "…" : p.t;
        // Keep labels inside the canvas so edge nodes stay readable.
        const half = ctx.measureText(label).width / 2 + 4;
        const lx = Math.min(Math.max(x, half), this.w - half);
        ctx.fillText(label, lx, y + this.radius(p) * this.nodeScale() + 3);
      });
      ctx.globalAlpha = 1;
    }
    bind() {
      const c = this.canvas;
      let start = null, moved = false, pinch = null;
      const pointers = new Map();
      const pos = (e) => { const r = c.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top]; };
      c.addEventListener("pointerdown", (e) => {
        pointers.set(e.pointerId, pos(e));
        c.setPointerCapture(e.pointerId);
        if (pointers.size === 2) {
          const [p1, p2] = [...pointers.values()];
          pinch = { d: Math.hypot(p1[0] - p2[0], p1[1] - p2[1]), scale: this.scale };
          this.drag = null;
          return;
        }
        const [x, y] = pos(e);
        const i = this.nodeAt(x, y);
        start = [x, y]; moved = false;
        this.drag = i >= 0 ? { node: i } : { pan: [x - this.tx, y - this.ty] };
        if (i >= 0) { this.nodes[i].fixed = true; this.kick(0.3); }
      });
      c.addEventListener("pointermove", (e) => {
        const [x, y] = pos(e);
        if (pointers.has(e.pointerId)) pointers.set(e.pointerId, [x, y]);
        if (pinch && pointers.size === 2) {
          const [p1, p2] = [...pointers.values()];
          this.scale = Math.min(8, Math.max(0.1, pinch.scale * Math.hypot(p1[0] - p2[0], p1[1] - p2[1]) / pinch.d));
          this.draw();
          return;
        }
        if (this.drag) {
          if (start && Math.hypot(x - start[0], y - start[1]) > 4) moved = true;
          if (this.drag.node !== undefined) {
            const [wx, wy] = this.toWorld(x, y);
            Object.assign(this.nodes[this.drag.node], { x: wx, y: wy });
            this.kick(0.3);
          } else {
            this.tx = x - this.drag.pan[0]; this.ty = y - this.drag.pan[1];
            this.draw();
          }
          return;
        }
        const h = this.nodeAt(x, y);
        if (h !== this.hover) { this.hover = h; c.style.cursor = h >= 0 ? "pointer" : "grab"; this.draw(); }
      });
      const end = (e) => {
        pointers.delete(e.pointerId);
        if (pointers.size < 2) pinch = null;
        if (!this.drag) return;
        const i = this.drag.node;
        if (i !== undefined) {
          this.nodes[i].fixed = false;
          if (!moved && e.type === "pointerup") {
            if (this.nodes[i].u) notCreated(this.nodes[i].t);
            else location.href = C.root + this.nodes[i].p;
          }
        }
        this.drag = null;
      };
      c.addEventListener("pointerup", end);
      c.addEventListener("pointercancel", end);
      c.addEventListener("pointerleave", () => { if (!this.drag && this.hover >= 0) { this.hover = -1; this.draw(); } });
      c.addEventListener("wheel", (e) => {
        e.preventDefault();
        const [x, y] = pos(e);
        const [wx, wy] = this.toWorld(x, y);
        this.scale = Math.min(8, Math.max(0.1, this.scale * Math.exp(-e.deltaY * 0.0015)));
        this.tx = x - this.w / 2 - wx * this.scale;
        this.ty = y - this.h / 2 - wy * this.scale;
        this.draw();
      }, { passive: false });
    }
  }

  function setupGraphs() {
    const current = data.notes.findIndex((n) => n.p === C.note);
    // Small slices show everything in the sidebar (with the current note
    // highlighted) so unlinked notes stay visible; large ones show neighbours only.
    const LOCAL_ONLY_ABOVE = 40;
    let nodes = data.notes, links = data.links, local = current;
    if (current >= 0 && data.notes.length > LOCAL_ONLY_ABOVE) {
      const keep = new Set([current]);
      for (const [a, b] of data.links) {
        if (a === current) keep.add(b);
        if (b === current) keep.add(a);
      }
      const ids = [...keep];
      const map = new Map(ids.map((id, i) => [id, i]));
      nodes = ids.map((id) => data.notes[id]);
      links = data.links.filter(([a, b]) => map.has(a) && map.has(b)).map(([a, b]) => [map.get(a), map.get(b)]);
      local = 0;
    }
    graphs.push(new Graph($("#graph-local"), nodes, links, local, nodes.length <= 30));

    let global = null;
    const modal = $("#graph-modal");
    const close = () => (modal.hidden = true);
    $("#graph-expand").addEventListener("click", () => {
      modal.hidden = false;
      if (!global) graphs.push((global = new Graph($("#graph-global"), data.notes, data.links, current, data.notes.length <= 30)));
      else { global.resize(); global.draw(); }
    });
    $("#graph-close").addEventListener("click", close);
    modal.addEventListener("click", (e) => { if (e.target === modal) close(); });
    document.addEventListener("keydown", (e) => { if (e.key === "Escape") close(); });
  }

  // ---------------------------------------------------------------- boot
  async function main() {
    if (C.encrypted) await unlock();
    enhanceContent();
    try {
      data = await loadData();
    } catch (e) {
      console.error("could not load site data", e);
      return;
    }
    buildTree();
    setupSearch();
    setupGraphs();
  }
  main();
})();
