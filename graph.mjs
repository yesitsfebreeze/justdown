#!/usr/bin/env node
// graph.mjs — build the justdown library graph. Single-file, zero-dependency.
//
//   node graph.mjs [libraryDir] [outFile]    scan .jd files → write graph.json
//
// Build-time only: this script regenerates graph.json from the library/ files and
// is run by CI on every push (see .github/workflows/graph.yml). Nothing needs it
// at runtime — the CLI is the pure-shell .jd/justfile, which queries the committed
// graph.json directly.
//
// The graph is a flat, autark index: every file is a SPARSE, QUANTIZED term-vector
// ("embed"). The vocabulary is dynamic — a key exists only if a file uses it. Keys
// are plain words, so the stored JSON reads back as named categories with no
// decoder. Files are addressed by raw git link, so consumers need no clone.

import { readFileSync, writeFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";

// ── config (env-overridable; defaults target this repo) ──────────────────────
const REPO     = process.env.JUSTDOWN_REPO     || "yesitsfebreeze/justdown";
const BRANCH   = process.env.JUSTDOWN_BRANCH   || "main";
const RAW_BASE = process.env.JUSTDOWN_RAW_BASE || `https://raw.githubusercontent.com/${REPO}/${BRANCH}`;
const LIB_DIR  = process.env.JUSTDOWN_LIB      || "library";
const QMAX     = 255;                                 // uint8 quantization ceiling

// ── tokenizer (shared by build + the justfile query → deterministic) ─────────
const STOP = new Set(
  ("a an the and or of to for in on at with without use used uses using when then " +
   "user asks ask this that it its into from by as is are be was were your you we our " +
   "any one run runs running file files thing things etc via per").split(/\s+/)
);
function tokens(text) {
  return (String(text).toLowerCase().match(/[a-z][a-z0-9+]{2,}/g) || []).filter(t => !STOP.has(t));
}

// ── tiny YAML frontmatter reader (key: scalar | [a, b]) ──────────────────────
function frontmatter(src) {
  const m = src.match(/^---\n([\s\S]*?)\n---/);
  const fm = {};
  if (!m) return fm;
  for (const line of m[1].split("\n")) {
    const mm = line.match(/^([A-Za-z_]+):\s*(.*)$/);
    if (!mm) continue;
    let [, k, v] = mm; v = v.trim();
    if (v.startsWith("[") && v.endsWith("]"))
      fm[k] = v.slice(1, -1).split(",").map(s => s.trim()).filter(Boolean);
    else fm[k] = v;
  }
  return fm;
}

// @links that reference another file: require a slash (drops shell `@echo`),
// keep only `dir/name`; unresolved targets drop at link time.
function atLinks(src) {
  const body = src.replace(/^---\n[\s\S]*?\n---/, "");
  const out = new Set();
  for (const m of body.matchAll(/@([a-z0-9_]+\/[a-z0-9_]+)(?:#[A-Za-z0-9_]+)?/g)) out.add(m[1]);
  return [...out];
}

// ── build: library/*.jd → graph.json ─────────────────────────────────────────
function walk(dir) {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out.push(...walk(p));
    else if (e.name.endsWith(".jd")) out.push(p);
  }
  return out;
}

function build(libDir, outFile) {
  const root = resolve(libDir);
  const files = walk(root).sort();
  if (!files.length) { console.error(`no .jd files under ${libDir}`); process.exit(1); }

  // 1) read files → distilled purpose + raw term list per file
  const docs = files.map(abs => {
    const raw = readFileSync(abs, "utf8");
    const fm = frontmatter(raw);
    const rel = relative(resolve("."), abs).split("\\").join("/"); // repo-relative, posix
    const key = rel.replace(/\.jd$/, "").split("/").slice(-2).join("/"); // dir/name
    const purpose = fm.description || fm.name || key;
    const terms = tokens(
      [fm.name, fm.name && fm.name.replace(/[/_-]/g, " "), purpose, (fm.tags || []).join(" ")].join(" ")
    );
    return {
      id: rel, key,
      name: fm.name || key,
      kind: fm.kind || "",
      tags: fm.tags || [],
      run: fm.run, invoke: fm.invoke || (fm.kind === "tool" ? "run" : undefined),
      provides: fm.provides || [],
      purpose,
      raw: `${RAW_BASE}/${rel}`,
      path: rel,
      terms,
      links: atLinks(raw),
    };
  });

  // 2) idf over the corpus
  const N = docs.length;
  const df = new Map();
  for (const d of docs) for (const t of new Set(d.terms)) df.set(t, (df.get(t) || 0) + 1);
  const idf = t => Math.log((N + 1) / ((df.get(t) || 0) + 1)) + 1;

  // 3) embed: tf·idf → quantize per-doc to 1..QMAX, stored sparse
  for (const d of docs) {
    const tf = new Map();
    for (const t of d.terms) tf.set(t, (tf.get(t) || 0) + 1);
    const rawW = new Map();
    let max = 0;
    for (const [t, f] of tf) { const w = f * idf(t); rawW.set(t, w); if (w > max) max = w; }
    const vec = {};
    for (const [t, w] of rawW) vec[t] = Math.max(1, Math.round((w / max) * QMAX));
    d.vec = vec;
  }

  // 4) edges: resolve @links against existing files (by dir/name suffix)
  const byKey = new Map(docs.map(d => [d.key, d]));
  const edges = [];
  for (const d of docs)
    for (const l of d.links) {
      const t = byKey.get(l);
      if (t && t.id !== d.id) edges.push({ from: d.id, to: t.id, ref: `@${l}` });
    }

  // 5) categories: group each file under its most-shared TAG (tags are the
  // curated vocabulary). Ties break by global frequency then alpha → stable.
  const tagDf = new Map();
  for (const d of docs) for (const t of d.tags) tagDf.set(t, (tagDf.get(t) || 0) + 1);
  const categories = {};
  for (const d of docs) {
    const seed = [...d.tags].sort((a, b) => (tagDf.get(b) - tagDf.get(a)) || a.localeCompare(b))[0];
    const cat = seed || d.kind || "misc";
    (categories[cat] ||= []).push(d.name);
  }
  const sortedCats = Object.fromEntries(
    Object.entries(categories).sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]))
      .map(([k, v]) => [k, v.sort()])
  );

  const nodes = docs.map(({ terms, links, ...n }) => n);
  const graph = {
    repo: REPO, branch: BRANCH, rawBase: RAW_BASE,
    note: "quantized sparse term-vectors (uint8); keys are the category vocabulary; scoring is integer dot-product",
    counts: { nodes: nodes.length, edges: edges.length, vocab: df.size, categories: Object.keys(sortedCats).length },
    categories: sortedCats, nodes, edges,
  };
  const out = outFile || "graph.json";
  writeFileSync(out, JSON.stringify(graph, null, 2) + "\n");
  console.error(`wrote ${out}: ${nodes.length} nodes, ${edges.length} edges, ${df.size} keys, ${Object.keys(categories).length} categories`);
}

// ── entry ────────────────────────────────────────────────────────────────────
// Accept both `graph.mjs [lib] [out]` and the legacy `graph.mjs --build [lib] [out]`.
const argv = process.argv.slice(2);
const a = argv[0] === "--build" ? argv.slice(1) : argv;
build(a[0] || LIB_DIR, a[1]);
