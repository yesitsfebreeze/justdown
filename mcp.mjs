#!/usr/bin/env node
// justdown — single-file, zero-dependency MCP server + graph builder.
//
//   node mcp.mjs --build [libraryDir] [outFile]   scan .jd files → write graph.json
//   node mcp.mjs                                   stdio MCP server over the graph
//
// The graph is a flat, autark index: every file is a SPARSE, QUANTIZED
// term-vector ("embed"). The vocabulary is dynamic — a key exists only if a
// file uses it (2 keys → 2 entries, nothing pre-allocated). Keys are plain
// words, so the stored JSON reads back as named categories with no decoder.
// Scoring is an integer dot-product: no model, no floats, no dependencies.
//
// Files are addressed by raw git link, so neither building nor querying needs a
// clone — `get` fetches a file body over HTTP (or from disk when local).

import { readFileSync, writeFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { join, dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));

// ── config (env-overridable; defaults target this repo) ──────────────────────
const REPO     = process.env.JUSTDOWN_REPO     || "yesitsfebreeze/justdown";
const BRANCH   = process.env.JUSTDOWN_BRANCH   || "main";
const RAW_BASE = process.env.JUSTDOWN_RAW_BASE || `https://raw.githubusercontent.com/${REPO}/${BRANCH}`;
const LIB_DIR  = process.env.JUSTDOWN_LIB      || "library";
const GRAPH    = process.env.JUSTDOWN_GRAPH    || ""; // explicit graph url/path; else auto
const QMAX     = 255;                                 // uint8 quantization ceiling

// ── tokenizer (shared by build + query → deterministic) ──────────────────────
const STOP = new Set(
  ("a an the and or of to for in on at with without use used uses using when then " +
   "user asks ask this that it its into from by as is are be was were your you we our " +
   "any one run runs running file files thing things etc via per") .split(/\s+/)
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
// keep only `dir/name`; unresolved targets (`@auth/crypto`) drop at link time.
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
    const src = frontmatterSrc(abs);
    const fm = frontmatter(src.raw);
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
      links: atLinks(src.raw),
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
    const raw = new Map();
    let max = 0;
    for (const [t, f] of tf) { const w = f * idf(t); raw.set(t, w); if (w > max) max = w; }
    const vec = {};
    for (const [t, w] of raw) vec[t] = Math.max(1, Math.round((w / max) * QMAX));
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
  // sort members + emit categories by size for a readable file
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

function frontmatterSrc(abs) { return { raw: readFileSync(abs, "utf8") }; }

// ── serve: stdio MCP over the graph ──────────────────────────────────────────
let GRAPH_CACHE = null;
async function loadGraph() {
  if (GRAPH_CACHE) return GRAPH_CACHE;
  const src =
    GRAPH ||
    (existsSync(join(HERE, "graph.json")) ? join(HERE, "graph.json") : `${RAW_BASE}/graph.json`);
  let text;
  if (/^https?:/.test(src)) { const r = await fetch(src); if (!r.ok) throw new Error(`graph fetch ${r.status} ${src}`); text = await r.text(); }
  else text = readFileSync(src, "utf8");
  GRAPH_CACHE = JSON.parse(text);
  GRAPH_CACHE.byName = new Map(GRAPH_CACHE.nodes.map(n => [n.name, n]));
  GRAPH_CACHE.byPath = new Map(GRAPH_CACHE.nodes.map(n => [n.path, n]));
  return GRAPH_CACHE;
}

function rankSearch(g, query, k = 5, kind) {
  const qtf = new Map();
  for (const t of tokens(query)) qtf.set(t, (qtf.get(t) || 0) + 1);
  const scored = g.nodes
    .filter(n => !kind || n.kind === kind)
    .map(n => {
      let s = 0;
      for (const [t, f] of qtf) if (n.vec[t]) s += f * n.vec[t]; // integer dot-product
      return { n, s };
    })
    .filter(x => x.s > 0)
    .sort((a, b) => b.s - a.s)
    .slice(0, k);
  return scored.map(({ n, s }) => ({
    name: n.name, kind: n.kind, purpose: n.purpose, tags: n.tags, raw: n.raw, score: s,
  }));
}

async function getFile(g, ref) {
  const n = g.byName.get(ref) || g.byPath.get(ref) || g.nodes.find(x => x.name === ref || x.path.endsWith(`/${ref}.jd`));
  if (!n) throw new Error(`no file: ${ref}`);
  const localGuess = resolve(HERE, n.path);
  let body;
  if (existsSync(localGuess) && statSync(localGuess).isFile()) body = readFileSync(localGuess, "utf8");
  else { const r = await fetch(n.raw); if (!r.ok) throw new Error(`fetch ${r.status} ${n.raw}`); body = await r.text(); }
  return { name: n.name, path: n.path, raw: n.raw, body };
}

function neighbors(g, ref) {
  const n = g.byName.get(ref) || g.byPath.get(ref);
  if (!n) throw new Error(`no file: ${ref}`);
  const link = id => { const t = g.byPath.get(id); return t ? { name: t.name, raw: t.raw } : { path: id }; };
  return {
    name: n.name,
    out: g.edges.filter(e => e.from === n.path).map(e => ({ ...link(e.to), ref: e.ref })),
    in:  g.edges.filter(e => e.to === n.path).map(e => ({ ...link(e.from), ref: e.ref })),
  };
}

// tool registry
const TOOLS = [
  { name: "search", description: "Search justdown files by purpose (integer term-vector dot-product). Returns ranked files with raw git links.",
    inputSchema: { type: "object", properties: { query: { type: "string" }, k: { type: "number", description: "max results (default 5)" }, kind: { type: "string", enum: ["tool", "agent", "knowledge", "workflow"] } }, required: ["query"] } },
  { name: "get", description: "Fetch a file's full .jd body by name or path (over the raw git link, or disk when local).",
    inputSchema: { type: "object", properties: { ref: { type: "string", description: "file name or path" } }, required: ["ref"] } },
  { name: "categories", description: "List the named, readable categories (vocabulary keys) and their member files.",
    inputSchema: { type: "object", properties: {} } },
  { name: "neighbors", description: "Outbound and inbound @links of a file.",
    inputSchema: { type: "object", properties: { ref: { type: "string" } }, required: ["ref"] } },
];

async function callTool(name, args) {
  const g = await loadGraph();
  switch (name) {
    case "search":     return rankSearch(g, args.query, args.k, args.kind);
    case "get":        return await getFile(g, args.ref);
    case "categories": return { categories: g.categories, counts: g.counts };
    case "neighbors":  return neighbors(g, args.ref);
    default: throw new Error(`unknown tool: ${name}`);
  }
}

// JSON-RPC 2.0 over newline-delimited stdio (MCP stdio transport)
function send(msg) { process.stdout.write(JSON.stringify(msg) + "\n"); }
async function handle(req) {
  const { id, method, params } = req;
  const reply = result => send({ jsonrpc: "2.0", id, result });
  const fail  = (code, message) => send({ jsonrpc: "2.0", id, error: { code, message } });
  try {
    switch (method) {
      case "initialize":
        return reply({ protocolVersion: "2024-11-05", capabilities: { tools: {} },
          serverInfo: { name: "justdown", version: "0.1.0" } });
      case "notifications/initialized": return; // notification, no reply
      case "ping": return reply({});
      case "tools/list": return reply({ tools: TOOLS });
      case "tools/call": {
        const out = await callTool(params.name, params.arguments || {});
        return reply({ content: [{ type: "text", text: JSON.stringify(out, null, 2) }] });
      }
      default:
        if (id !== undefined) fail(-32601, `method not found: ${method}`);
    }
  } catch (e) {
    if (id !== undefined) fail(-32603, String(e && e.message || e));
  }
}

function serve() {
  let buf = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", chunk => {
    buf += chunk;
    let nl;
    while ((nl = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, nl).trim();
      buf = buf.slice(nl + 1);
      if (line) { try { handle(JSON.parse(line)); } catch { /* skip non-JSON line */ } }
    }
  });
  process.stdin.on("end", () => process.exit(0));
  console.error("justdown mcp: ready (stdio)");
}

// ── entry ────────────────────────────────────────────────────────────────────
const argv = process.argv.slice(2);
if (argv[0] === "--build") build(argv[1] || LIB_DIR, argv[2]);
else serve();
