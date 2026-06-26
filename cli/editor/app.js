import { EditorView, Decoration, ViewPlugin, WidgetType, keymap, drawSelection,
         placeholder } from "@codemirror/view";
import { EditorState, StateField, StateEffect } from "@codemirror/state";
import { syntaxTree } from "@codemirror/language";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { SearchCursor } from "@codemirror/search";
import { history, defaultKeymap, historyKeymap,
         cursorGroupLeft, cursorGroupRight,
         selectGroupLeft, selectGroupRight } from "@codemirror/commands";
import { markdownTable } from "markdown-table";

/* ================================================================== *
 *  iA Writer-style live preview for .jd files (markdown + frontmatter
 *  + {{variables}}). Decorates the REAL editor text:
 *    - markdown formatting rendered inline (bold, headings, code…)
 *    - syntax markers (**, #, `) stay visible but dimmed to gray
 *    - YAML frontmatter block gets its own quiet styling
 *    - {{var}} / <<var>> template slots render as pills
 *    - the paragraph under the cursor focuses; the rest gently fades
 * ================================================================== */

const HEADING_CLASS = {
  ATXHeading1: "cm-h1", ATXHeading2: "cm-h2", ATXHeading3: "cm-h3",
  ATXHeading4: "cm-h4", ATXHeading5: "cm-h5", ATXHeading6: "cm-h6",
};
const INLINE_CLASS = {
  StrongEmphasis: "cm-strong", Emphasis: "cm-em",
  Strikethrough: "cm-strike", InlineCode: "cm-inline-code", Link: "cm-link",
};
const SYNTAX_NODES = new Set([
  "HeaderMark", "EmphasisMark", "CodeMark", "StrikethroughMark",
  "QuoteMark", "LinkMark", "URL",
]);

const VAR_RE = /\{\{[^}\n]+\}\}|<<[^>\n]+>>/g;
const FM_KEY_RE = /^(\s*)([\w-]+)(\s*:)/;
// GFM tables in these .jd files use leading+trailing pipes, so "starts and
// ends with |" is a reliable row test that won't catch shell pipes in prose.
const TABLE_ROW_RE = /^\s*\|.*\|\s*$/;
const TABLE_SEP_RE = /^\s*\|[\s:|-]*-[\s:|-]*\|?\s*$/;

// Renders a real bullet glyph in place of a hidden "-"/"*"/"+" list marker.
class BulletWidget extends WidgetType {
  eq() { return true; }
  toDOM() {
    const s = document.createElement("span");
    s.className = "cm-bullet";
    s.textContent = "•";
    return s;
  }
}
const bulletDeco = Decoration.replace({ widget: new BulletWidget() });

// Renders a YAML array frontmatter value (e.g. [nu, data, core]) as chips.
class TagsWidget extends WidgetType {
  constructor(tags) { super(); this.tags = tags; }
  eq(o) { return o.tags.join("") === this.tags.join(""); }
  toDOM() {
    const wrap = document.createElement("span");
    wrap.className = "cm-tags";
    for (const t of this.tags) {
      const chip = document.createElement("span");
      chip.className = "cm-tag";
      chip.textContent = t;
      wrap.appendChild(chip);
    }
    return wrap;
  }
}

function activeLineSet(state) {
  const lines = new Set();
  for (const r of state.selection.ranges) {
    const a = state.doc.lineAt(r.from).number;
    const b = state.doc.lineAt(r.to).number;
    for (let i = a; i <= b; i++) lines.add(i);
  }
  return lines;
}

// Block-level node names — the structural levels a cursor can sit "inside".
const BLOCK_NODES = new Set([
  "Paragraph", "ListItem", "Blockquote", "FencedCode", "CodeBlock",
  "ATXHeading1", "ATXHeading2", "ATXHeading3", "ATXHeading4", "ATXHeading5", "ATXHeading6",
  "SetextHeading1", "SetextHeading2", "Table", "HTMLBlock", "CommentBlock",
]);

// The set of line numbers belonging to the DEEPEST block the cursor sits in
// (the innermost paragraph / list item / blockquote / code block / heading /
// table). Focus mode keeps exactly this block bright and dims everything else,
// so a multi-line block (e.g. a code fence) stays fully lit while you edit it.
function activeBlockLines(state) {
  const set = new Set();
  const tree = syntaxTree(state);
  for (const r of state.selection.ranges) {
    // light every line the selection itself covers (so a selection is never
    // half-dimmed), then add the deepest block enclosing the caret head.
    const la = state.doc.lineAt(r.from).number;
    const lb = state.doc.lineAt(r.to).number;
    for (let i = la; i <= lb; i++) set.add(i);
    // deepest level = the SMALLEST enclosing block. Check both sides of the
    // caret so a boundary position doesn't resolve to an outer container
    // (e.g. a whole blockquote instead of the list item nested inside it).
    let block = null;
    for (const side of [-1, 1]) {
      for (let n = tree.resolveInner(r.head, side); n; n = n.parent) {
        if (BLOCK_NODES.has(n.name)) {
          if (!block || (n.to - n.from) < (block.to - block.from)) block = n;
          break;
        }
      }
    }
    if (block) {
      const a = state.doc.lineAt(block.from).number;
      const b = state.doc.lineAt(Math.min(block.to, state.doc.length)).number;
      for (let i = a; i <= b; i++) set.add(i);
    }
  }
  return set;
}

// True when any selection range touches [from, to] (inclusive). Drives
// per-token markdown reveal: a span shows its raw markers only when the
// cursor is actually inside (or right at the edge of) that span — every
// other token on the same line stays rendered, the Obsidian Live Preview feel.
function cursorTouches(state, from, to) {
  for (const r of state.selection.ranges) {
    if (r.from <= to && r.to >= from) return true;
  }
  return false;
}

/* ------------------------------------------------------------------ *
 *  GFM tables: rendered as real aligned <table>s when the cursor is
 *  outside the block, and revealed as raw source (for editing) when
 *  the cursor is inside — the Obsidian Live Preview pattern. Block
 *  decorations must come from a StateField, hence the separate field.
 * ------------------------------------------------------------------ */

function findTableBlocks(state) {
  const blocks = [];
  const N = state.doc.lines;
  const fm = frontmatter(state);
  const fmClose = fm ? fm.close : 0;
  let ln = 1;
  while (ln < N) {
    if (ln > fmClose &&
        TABLE_ROW_RE.test(state.doc.line(ln).text) &&
        TABLE_SEP_RE.test(state.doc.line(ln + 1).text)) {
      let end = ln + 1;
      while (end + 1 <= N) {
        const t = state.doc.line(end + 1).text;
        if (TABLE_ROW_RE.test(t) && !TABLE_SEP_RE.test(t)) end++;
        else break;
      }
      blocks.push({ from: ln, to: end });
      ln = end + 1;
    } else ln++;
  }
  return blocks;
}

function blockActive(b, activeSet) {
  for (let ln = b.from; ln <= b.to; ln++) if (activeSet.has(ln)) return true;
  return false;
}

function inactiveTableLines(state) {
  const set = new Set();
  const activeSet = activeLineSet(state);
  for (const b of findTableBlocks(state)) {
    if (blockActive(b, activeSet) || isRawBlock(state, b)) continue;
    for (let ln = b.from; ln <= b.to; ln++) set.add(ln);
  }
  return set;
}

function splitRow(text) {
  let s = text.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
  // split on unescaped pipes, then unescape `\|` → `|` so a literal pipe in a
  // cell round-trips through the editor instead of fracturing the column count.
  return s.split(/(?<!\\)\|/).map((c) => c.trim().replace(/\\\|/g, "|"));
}

function parseTableBlock(state, fromLine, toLine) {
  const rows = [];
  let aligns = [];
  for (let ln = fromLine; ln <= toLine; ln++) {
    const text = state.doc.line(ln).text;
    if (ln === fromLine + 1) {
      aligns = splitRow(text).map((c) => {
        const t = c.trim();
        const l = t.startsWith(":"), r = t.endsWith(":");
        return l && r ? "center" : r ? "right" : l ? "left" : "";
      });
    } else {
      rows.push(splitRow(text));
    }
  }
  return { rows, aligns };
}

function renderCell(text) {
  let h = text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
  return h
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<span class="cm-link">$1</span>') // link → styled text
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/(^|[^*])\*([^*\s][^*]*)\*/g, "$1<em>$2</em>")               // italic (not **)
    .replace(/(\{\{[^}\n]+\}\})/g, '<span class="cm-variable">$1</span>');
}

/* GFM serialization is delegated to `markdown-table` (the remark/unified
 * library) so the written-back source is always cleanly padded and aligned —
 * the one part of "table editing" with a canonical, battle-tested solution. */
const ALIGN_MT = { left: "l", center: "c", right: "r", "": null };
function serializeTable(rows, aligns) {
  const esc = rows.map((row) => row.map((c) => String(c).replace(/\|/g, "\\|")));
  return markdownTable(esc, { align: aligns.map((a) => ALIGN_MT[a] ?? null) });
}

// After a structural edit re-renders the widget, restore the caret to a
// sensible cell. Only one table is edited at a time, so a module-level handoff
// is enough: the re-rendered widget's toDOM consumes it.
let pendingFocus = null;

/* Per-table "show the raw markdown" toggle. We remember which tables the user
 * flipped to source view as a set of doc positions (one inside each such
 * block) and map them through every edit so the choice survives typing. A
 * block whose range contains a remembered position renders as plain editable
 * markdown instead of the interactive widget. */
const setRawTable = StateEffect.define();   // { on: bool, from?, lo?, hi? }
const rawTablesField = StateField.define({
  create() { return []; },
  update(val, tr) {
    let next = tr.docChanged
      ? val.map((p) => tr.changes.mapPos(p, 1)).filter((p) => p != null)
      : val;
    for (const e of tr.effects) {
      if (!e.is(setRawTable)) continue;
      if (e.value.on) {
        if (!next.includes(e.value.from)) next = [...next, e.value.from];
      } else {
        next = next.filter((p) => p < e.value.lo || p > e.value.hi);
      }
    }
    return next;
  },
});
function isRawBlock(state, b) {
  const marks = state.field(rawTablesField, false);
  if (!marks || !marks.length) return false;
  const lo = state.doc.line(b.from).from, hi = state.doc.line(b.to).to;
  return marks.some((p) => p >= lo && p <= hi);
}

function placeCaretEnd(el) {
  const r = document.createRange();
  r.selectNodeContents(el);
  r.collapse(false);
  const sel = window.getSelection();
  sel.removeAllRanges();
  sel.addRange(r);
}

// Is the caret at the very start / end of a cell's editable text?
function caretAtEnd(el) {
  const sel = window.getSelection();
  if (!sel.rangeCount) return true;
  const range = sel.getRangeAt(0);
  if (!range.collapsed) return false;
  const after = document.createRange();
  after.selectNodeContents(el);
  after.setStart(range.endContainer, range.endOffset);
  return after.toString().length === 0;
}
function caretAtStart(el) {
  const sel = window.getSelection();
  if (!sel.rangeCount) return true;
  const range = sel.getRangeAt(0);
  if (!range.collapsed) return false;
  const before = document.createRange();
  before.selectNodeContents(el);
  before.setEnd(range.startContainer, range.startOffset);
  return before.toString().length === 0;
}

// Move keyboard focus into a rendered table's cell — used when the CM caret
// arrows into a table from the surrounding prose. `r`/`c` accept "last" to mean
// the final row/column. Returns true if a cell was focused.
function focusTableCell(view, fromPos, r, c) {
  const wrap = view.dom.querySelector(`.cm-md-table-wrap[data-from="${fromPos}"]`);
  if (!wrap) return false;
  const maxOf = (attr, within) =>
    Math.max(0, ...[...(within || wrap).querySelectorAll(`[data-${attr}]`)].map((el) => +el.dataset[attr]));
  const rr = r === "last" ? maxOf("r") : r;
  let cc = c;
  if (c === "last") {
    const row = wrap.querySelector(`[data-r="${rr}"]`)?.parentElement;
    cc = maxOf("c", row);
  }
  const cell = wrap.querySelector(`[data-r="${rr}"][data-c="${cc}"]`) ||
               wrap.querySelector(`[data-r="${rr}"]`);
  if (cell) { cell.focus(); return true; }
  return false;
}

// Keymap handlers: arrowing the CM caret toward a rendered table steps INTO it
// (focusing a cell) instead of skipping over the block. Raw-view tables are
// left to normal text motion. `edge` checks the caret sits where the key would
// cross into the table; `entry` decides which cell to land on.
function enterTable(view, pick, entry) {
  const s = view.state.selection.main;
  if (!s.empty) return false;
  const ln = view.state.doc.lineAt(s.head);
  const b = pick(view.state, ln, s);
  if (!b || isRawBlock(view.state, b)) return false;
  return focusTableCell(view, view.state.doc.line(b.from).from, entry.r, entry.c);
}
const tableBelow = (st, ln) => findTableBlocks(st).find((b) => b.from === ln.number + 1);
const tableAbove = (st, ln) => findTableBlocks(st).find((b) => b.to === ln.number - 1);
const tableNavKeymap = [
  { key: "ArrowDown", run: (v) => enterTable(v, tableBelow, { r: 0, c: 0 }) },
  { key: "ArrowUp", run: (v) => enterTable(v, tableAbove, { r: "last", c: 0 }) },
  { key: "ArrowRight", run: (v) => enterTable(v, (st, ln, s) => (s.head === ln.to ? tableBelow(st, ln) : null), { r: 0, c: 0 }) },
  { key: "ArrowLeft", run: (v) => enterTable(v, (st, ln, s) => (s.head === ln.from ? tableAbove(st, ln) : null), { r: "last", c: "last" }) },
];

class TableWidget extends WidgetType {
  // `rows` is [header, ...dataRows] (the separator line is parsed into
  // `aligns`); from/to are the block's source char range for write-back.
  constructor(rows, aligns, from, to) {
    super();
    this.rows = rows;
    this.aligns = aligns;
    this.from = from;
    this.to = to;
  }
  eq(o) {
    return o.from === this.from &&
           JSON.stringify(o.rows) === JSON.stringify(this.rows) &&
           JSON.stringify(o.aligns) === JSON.stringify(this.aligns);
  }
  ignoreEvent() { return true; }   // CM ignores all events from the widget DOM
  toDOM(view) {
    // Working copy: every interaction mutates `work`/`workAligns`; we only
    // dispatch a CM transaction (re-serializing the whole block) when focus
    // leaves the table or a structural op fires. CM's WidgetView ignores DOM
    // mutations inside the widget, so contentEditable cells are safe.
    const work = this.rows.map((r) => r.slice());
    const workAligns = this.aligns.slice();
    const baseline = serializeTable(this.rows, this.aligns);
    const cells = [];                 // parallel 2D grid of cell elements
    let activeCell = null;            // {r, c} of the focused cell

    const wrap = document.createElement("div");
    wrap.className = "cm-md-table-wrap";
    wrap.contentEditable = "false";   // atomic island inside CM's content
    wrap.dataset.from = String(this.from);   // lets arrow-into-table find this widget

    const dispatch = (focus) => {
      const text = serializeTable(work, workAligns);
      if (text === view.state.sliceDoc(this.from, this.to)) {
        if (focus) { pendingFocus = focus; rerenderFocus(); }
        return;
      }
      pendingFocus = focus || null;
      view.dispatch({ changes: { from: this.from, to: this.to, insert: text } });
    };
    // No source change but we still want to move the caret (e.g. a no-op align).
    const rerenderFocus = () => {
      if (!pendingFocus) return;
      const { r, c } = pendingFocus; pendingFocus = null;
      const el = cells[r]?.[c];
      if (el) { el.focus(); placeCaretEnd(el); }
    };

    const syncActive = () => {
      if (!activeCell) return;
      const el = cells[activeCell.r]?.[activeCell.c];
      if (el && el.isContentEditable) stashCell(el, activeCell.r, activeCell.c);
    };
    const stashCell = (el, r, c) => {
      work[r][c] = el.textContent.replace(/ /g, " ").replace(/\n/g, " ").trim();
      el.innerHTML = renderCell(work[r][c]) || "<br>";
    };

    // ---- structural ops (operate on the working model, then commit) ----
    const cols = () => workAligns.length;
    const insertCol = (at) => { syncActive(); work.forEach((row) => row.splice(at, 0, "")); workAligns.splice(at, 0, ""); dispatch({ r: activeCell?.r ?? 0, c: at }); };
    const deleteCol = (at) => { if (cols() <= 1) return; syncActive(); work.forEach((row) => row.splice(at, 1)); workAligns.splice(at, 1); dispatch({ r: activeCell?.r ?? 0, c: Math.min(at, cols() - 1) }); };
    const insertRow = (at) => { at = Math.max(1, Math.min(at, work.length)); syncActive(); work.splice(at, 0, new Array(cols()).fill("")); dispatch({ r: at, c: activeCell?.c ?? 0 }); };
    const deleteRow = (at) => { if (at < 1 || work.length <= 1) return; syncActive(); work.splice(at, 1); dispatch({ r: Math.min(at, work.length - 1), c: activeCell?.c ?? 0 }); };
    const setAlign = (col, val) => { syncActive(); workAligns[col] = workAligns[col] === val ? "" : val; dispatch({ r: activeCell?.r ?? 0, c: col }); };

    // ---- contextual toolbar (shown while a cell is focused) ----
    const toolbar = document.createElement("div");
    toolbar.className = "cm-tbl-toolbar";
    const mkBtn = (label, title, fn, group) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "cm-tbl-btn" + (group ? " " + group : "");
      b.textContent = label;
      b.title = title;
      // mousedown+preventDefault keeps the focused cell focused (no blur churn)
      b.addEventListener("mousedown", (e) => { e.preventDefault(); e.stopPropagation(); fn(); });
      return b;
    };
    const sep = () => { const s = document.createElement("span"); s.className = "cm-tbl-sep"; return s; };
    toolbar.append(
      mkBtn("↥", "Insert row above", () => deleteSafeRowAbove()),
      mkBtn("↧", "Insert row below", () => insertRow((activeCell?.r ?? 0) + 1)),
      mkBtn("⊖", "Delete row", () => deleteRow(activeCell?.r ?? -1), "danger"),
      sep(),
      mkBtn("⇤", "Insert column left", () => insertCol(activeCell?.c ?? 0)),
      mkBtn("⇥", "Insert column right", () => insertCol((activeCell?.c ?? 0) + 1)),
      mkBtn("⊖", "Delete column", () => deleteCol(activeCell?.c ?? -1), "danger"),
      sep(),
      mkBtn("⇤", "Align left", () => setAlign(activeCell?.c ?? 0, "left"), "align"),
      mkBtn("⇔", "Align center", () => setAlign(activeCell?.c ?? 0, "center"), "align"),
      mkBtn("⇥", "Align right", () => setAlign(activeCell?.c ?? 0, "right"), "align"),
      sep(),
      mkBtn("</>", "Edit as markdown", () => {
        syncActive();
        if (serializeTable(work, workAligns) !== baseline) dispatch(null);
        view.dispatch({ effects: setRawTable.of({ on: true, from: this.from }) });
      }, "wide"),
    );
    const deleteSafeRowAbove = () => insertRow(Math.max(1, activeCell?.r ?? 1));
    const reflectAlign = () => {
      const a = activeCell ? workAligns[activeCell.c] : "";
      const al = toolbar.querySelectorAll(".cm-tbl-btn.align");
      ["left", "center", "right"].forEach((v, i) => al[i]?.classList.toggle("on", a === v));
    };

    // ---- build the table ----
    const table = document.createElement("table");
    table.className = "cm-md-table";
    // Leave the table via the keyboard, dropping the CM caret onto the line
    // just before/after the source block (and committing any pending edits).
    const exitTable = (where) => {
      if (serializeTable(work, workAligns) !== baseline) dispatch(null);
      const doc = view.state.doc;
      const endNo = doc.lineAt(Math.min(this.to, doc.length)).number;
      const startNo = doc.lineAt(this.from).number;
      const target = where === "after"
        ? (endNo < doc.lines ? doc.line(endNo + 1).from : doc.line(endNo).to)
        : (startNo > 1 ? doc.line(startNo - 1).to : 0);
      view.dispatch({ selection: { anchor: target } });
      view.focus();
    };
    const mkCell = (tag, r, c) => {
      const el = document.createElement(tag);
      el.className = "cm-tcell";
      el.contentEditable = "true";
      el.spellcheck = true;
      el.dataset.r = r;
      el.dataset.c = c;
      el.innerHTML = renderCell(work[r][c]) || "<br>";
      if (workAligns[c]) el.style.textAlign = workAligns[c];
      el.addEventListener("focus", () => {
        activeCell = { r, c };
        el.textContent = work[r][c];      // reveal raw markdown for editing
        placeCaretEnd(el);
        wrap.classList.add("editing");
        reflectAlign();
      });
      el.addEventListener("blur", (e) => {
        stashCell(el, r, c);
        const to = e.relatedTarget;
        if (to && wrap.contains(to)) return;   // moving within the table
        activeCell = null;
        wrap.classList.remove("editing");
        if (serializeTable(work, workAligns) !== baseline) dispatch(null);
      });
      el.addEventListener("keydown", (e) => {
        if (e.key === "Enter" && !e.shiftKey) {
          e.preventDefault();
          stashCell(el, r, c);
          const next = cells[r + 1]?.[c];
          if (next) next.focus(); else el.blur();
        } else if (e.key === "Tab") {
          e.preventDefault();
          stashCell(el, r, c);
          const next = e.shiftKey
            ? (cells[r]?.[c - 1] || cells[r - 1]?.[cols() - 1])
            : (cells[r]?.[c + 1] || cells[r + 1]?.[0]);
          if (next) next.focus(); else if (!e.shiftKey) insertCol(cols());
        } else if (e.key === "Escape") {
          e.preventDefault();
          el.innerHTML = renderCell(work[r][c]) || "<br>";
          el.blur();
        } else if (e.key === "ArrowDown") {
          e.preventDefault();
          stashCell(el, r, c);
          const n = cells[r + 1]?.[c];
          if (n) n.focus(); else exitTable("after");
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          stashCell(el, r, c);
          const p = cells[r - 1]?.[c];
          if (p) p.focus(); else exitTable("before");
        } else if (e.key === "ArrowRight" && caretAtEnd(el)) {
          e.preventDefault();
          stashCell(el, r, c);
          const n = cells[r]?.[c + 1] || cells[r + 1]?.[0];
          if (n) n.focus(); else exitTable("after");
        } else if (e.key === "ArrowLeft" && caretAtStart(el)) {
          e.preventDefault();
          stashCell(el, r, c);
          const p = cells[r]?.[c - 1] || cells[r - 1]?.[cols() - 1];
          if (p) p.focus(); else exitTable("before");
        }
      });
      return el;
    };

    const thead = document.createElement("thead");
    const htr = document.createElement("tr");
    cells[0] = [];
    (work[0] || []).forEach((_, c) => { const el = mkCell("th", 0, c); cells[0][c] = el; htr.appendChild(el); });
    thead.appendChild(htr);
    table.appendChild(thead);
    const tbody = document.createElement("tbody");
    for (let r = 1; r < work.length; r++) {
      const tr = document.createElement("tr");
      cells[r] = [];
      work[r].forEach((_, c) => { const el = mkCell("td", r, c); cells[r][c] = el; tr.appendChild(el); });
      tbody.appendChild(tr);
    }
    table.appendChild(tbody);

    // ---- always-available append affordances (visible on table hover) ----
    const addCol = document.createElement("button");
    addCol.type = "button";
    addCol.className = "cm-tbl-add cm-tbl-add-col";
    addCol.title = "Add column";
    addCol.textContent = "+";
    addCol.addEventListener("mousedown", (e) => { e.preventDefault(); insertCol(cols()); });
    const addRow = document.createElement("button");
    addRow.type = "button";
    addRow.className = "cm-tbl-add cm-tbl-add-row";
    addRow.title = "Add row";
    addRow.textContent = "+";
    addRow.addEventListener("mousedown", (e) => { e.preventDefault(); insertRow(work.length); });

    const grid = document.createElement("div");
    grid.className = "cm-tbl-grid";
    grid.append(table, addCol);
    wrap.append(toolbar, grid, addRow);

    // restore caret after a structural re-render
    if (pendingFocus) {
      const want = pendingFocus; pendingFocus = null;
      requestAnimationFrame(() => {
        const el = cells[want.r]?.[want.c] || cells[want.r]?.[cols() - 1] || cells[0]?.[0];
        if (el) { el.focus(); placeCaretEnd(el); }
      });
    }
    return wrap;
  }
}

const tableField = StateField.define({
  create(state) { return buildTableDecos(state); },
  update(value, tr) {
    const rawToggled = tr.effects.some((e) => e.is(setRawTable));
    return (tr.docChanged || tr.selection || rawToggled) ? buildTableDecos(tr.state) : value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

function buildTableDecos(state) {
  const activeSet = activeLineSet(state);
  const decos = [];
  for (const b of findTableBlocks(state)) {
    if (blockActive(b, activeSet)) continue;
    const fromPos = state.doc.line(b.from).from;
    const toPos = state.doc.line(b.to).to;
    if (isRawBlock(state, b)) {
      // raw markdown view: leave the source lines visible & editable, just
      // float a slim "rendered table" bar above them to toggle back.
      decos.push(Decoration.widget({ block: true, side: -1, widget: new RawBarWidget(fromPos, toPos) }).range(fromPos));
      continue;
    }
    const { rows, aligns } = parseTableBlock(state, b.from, b.to);
    decos.push(Decoration.replace({ block: true, widget: new TableWidget(rows, aligns, fromPos, toPos) }).range(fromPos, toPos));
  }
  return Decoration.set(decos, true);
}

// Slim bar shown above a table that's flipped to raw-markdown view.
class RawBarWidget extends WidgetType {
  constructor(from, to) { super(); this.from = from; this.to = to; }
  eq(o) { return o.from === this.from && o.to === this.to; }
  ignoreEvent() { return true; }
  toDOM(view) {
    const bar = document.createElement("div");
    bar.className = "cm-tbl-rawbar";
    bar.contentEditable = "false";
    const label = document.createElement("span");
    label.className = "cm-tbl-rawbar-label";
    label.textContent = "markdown source";
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "cm-tbl-rawbar-btn";
    btn.textContent = "⊞ rendered table";
    btn.title = "Show the rendered, editable table";
    btn.addEventListener("mousedown", (e) => {
      e.preventDefault();
      view.dispatch({ effects: setRawTable.of({ on: false, lo: this.from, hi: this.to }) });
    });
    bar.append(label, btn);
    return bar;
  }
}

function eachLine(state, from, to, fn) {
  let pos = from;
  while (pos <= to) {
    const line = state.doc.lineAt(pos);
    fn(line);
    if (line.to + 1 <= pos) break;
    pos = line.to + 1;
  }
}

// Leading `---` … `---` YAML block, if present.
function frontmatter(state) {
  if (state.doc.lines < 2 || state.doc.line(1).text.trim() !== "---") return null;
  for (let i = 2; i <= state.doc.lines; i++) {
    const t = state.doc.line(i).text.trim();
    if (t === "---" || t === "...") return { open: 1, close: i };
  }
  return null;
}

function buildDecorations(view) {
  const { state } = view;
  const active = activeLineSet(state);          // per-line — drives marker reveal
  const dimOn = activeBlockLines(state);        // deepest block — drives focus dim
  const fm = frontmatter(state);
  const tableSkip = inactiveTableLines(state); // lines rendered as <table> widgets
  const decos = [];

  for (const { from, to } of view.visibleRanges) {
    eachLine(state, from, to, (line) => {
      if (tableSkip.has(line.number)) return; // replaced by a block table widget
      // focus fade — keep the deepest block bright, dim everything else
      if (!dimOn.has(line.number)) {
        decos.push(Decoration.line({ class: "cm-dim" }).range(line.from));
      }
      // frontmatter → rendered as an individual-fields table.
      if (fm && line.number >= fm.open && line.number <= fm.close) {
        const onLine = active.has(line.number);
        const fence = line.number === fm.open || line.number === fm.close;
        if (fence) {
          decos.push(Decoration.line({ class: "cm-fm cm-fm-fence" }).range(line.from));
          // hide the `---` delimiters unless the cursor is on them
          if (!onLine && line.to > line.from) {
            decos.push(Decoration.replace({}).range(line.from, line.to));
          }
        } else {
          decos.push(Decoration.line({ class: "cm-fm cm-fm-field" }).range(line.from));
          const m = FM_KEY_RE.exec(line.text);
          if (m) {
            const ks = line.from + m[1].length;
            const ke = ks + m[2].length;            // end of key word
            const afterKey = line.from + m[0].length; // end of `key:` (incl. colon)
            if (onLine) {
              // editing this field → show raw `key: value`
              decos.push(Decoration.mark({ class: "cm-fm-key" }).range(ks, ke));
            } else {
              // rendered field: key becomes a column label, colon hidden
              decos.push(Decoration.mark({ class: "cm-fm-label" }).range(ks, ke));
              let end = afterKey;
              if (state.doc.sliceString(afterKey, afterKey + 1) === " ") end += 1;
              if (end > ke) decos.push(Decoration.replace({}).range(ke, end));
              const valueText = line.text.slice(end - line.from);
              const am = /^\s*\[(.+)\]\s*$/.exec(valueText);
              if (am && end < line.to) {
                // array value → tag chips
                const tags = am[1].split(",")
                  .map((t) => t.trim().replace(/^["']|["']$/g, ""))
                  .filter(Boolean);
                if (tags.length) {
                  decos.push(Decoration.replace({ widget: new TagsWidget(tags) }).range(end, line.to));
                }
              } else if (/^(kind|status|type)$/i.test(m[2]) && valueText.trim() && end < line.to) {
                // enum-like value → subtle badge
                decos.push(Decoration.mark({ class: "cm-fm-badge" }).range(end, line.to));
              }
            }
          }
        }
      }
      // markdown tables → monospace so columns align, with dimmed pipes
      const inFm = fm && line.number >= fm.open && line.number <= fm.close;
      if (!inFm && TABLE_ROW_RE.test(line.text)) {
        const isSep = TABLE_SEP_RE.test(line.text);
        decos.push(Decoration.line({ class: isSep ? "cm-table cm-table-sep" : "cm-table" }).range(line.from));
        if (!isSep) {
          for (let i = 0; i < line.text.length; i++) {
            if (line.text[i] === "|") {
              decos.push(Decoration.mark({ class: "cm-tpipe" }).range(line.from + i, line.from + i + 1));
            }
          }
        }
      }
      // {{variables}} / <<variables>> (anywhere)
      VAR_RE.lastIndex = 0;
      let mm;
      while ((mm = VAR_RE.exec(line.text))) {
        const s = line.from + mm.index;
        decos.push(Decoration.mark({ class: "cm-variable" }).range(s, s + mm[0].length));
      }
    });

    syntaxTree(state).iterate({
      from, to,
      enter: (node) => {
        const name = node.name, nf = node.from, nt = node.to;
        if (fm && state.doc.lineAt(nf).number <= fm.close) return; // skip md parsing inside frontmatter
        if (tableSkip.has(state.doc.lineAt(nf).number)) return;    // inside a rendered table

        if (HEADING_CLASS[name]) {
          decos.push(Decoration.line({ class: HEADING_CLASS[name] }).range(state.doc.lineAt(nf).from));
        } else if (name === "Blockquote") {
          eachLine(state, nf, nt, (l) => decos.push(Decoration.line({ class: "cm-quote" }).range(l.from)));
        } else if (name === "FencedCode" || name === "CodeBlock") {
          const startLine = state.doc.lineAt(nf).number;
          const endLine = state.doc.lineAt(nt).number;
          let n = 0;
          for (let ln = startLine; ln <= endLine; ln++) {
            const line = state.doc.line(ln);
            const isOpen = name === "FencedCode" && ln === startLine;
            const isClose = name === "FencedCode" && ln === endLine;
            if (isOpen || isClose) {
              const edge = isOpen ? " cm-code-open" : " cm-code-close";
              decos.push(Decoration.line({ class: "cm-code-line cm-code-fence" + edge }).range(line.from));
            } else {
              n++;
              // highlight the line number with the accent color when the
              // cursor sits on this line inside the code block.
              const onLine = active.has(ln);
              decos.push(Decoration.line({
                class: "cm-code-line cm-code-num" + (onLine ? " cm-code-num-active" : ""),
                attributes: { "data-ln": String(n) },
              }).range(line.from));
            }
          }
        } else if (name === "Link" && nt > nf) {
          // attach the target so Cmd/Ctrl+click can follow it
          const m = /\]\(([^)\s]+)/.exec(state.doc.sliceString(nf, nt));
          decos.push(Decoration.mark({
            class: "cm-link",
            attributes: m ? { "data-href": m[1] } : undefined,
          }).range(nf, nt));
        } else if (INLINE_CLASS[name] && nt > nf) {
          decos.push(Decoration.mark({ class: INLINE_CLASS[name] }).range(nf, nt));
        } else if (name === "ListMark" && nt > nf) {
          // keep list items legible: show the editable "-" (dimmed) on the
          // cursor line, render a real bullet glyph everywhere else.
          const on = active.has(state.doc.lineAt(nf).number);
          const isUnordered = /^[-*+]$/.test(state.doc.sliceString(nf, nt));
          if (!on && isUnordered) decos.push(bulletDeco.range(nf, nt));
          else decos.push(Decoration.mark({ class: "cm-syntax" }).range(nf, nt));
        } else if (SYNTAX_NODES.has(name) && nt > nf) {
          // Obsidian-style inline editing. Block markers (heading #, quote >)
          // reveal when the cursor is anywhere on their line; inline markers
          // (**, *, ~~, `, link [](…)) reveal ONLY when the cursor is inside
          // that specific span — so every other token on the line stays
          // rendered. Fence markers (```) always stay visible.
          const isFence = name === "CodeMark" && nt - nf >= 3;
          const blockMark = name === "HeaderMark" || name === "QuoteMark";
          let revealed;
          if (blockMark) {
            revealed = active.has(state.doc.lineAt(nf).number);
          } else {
            const parent = node.node.parent;
            revealed = parent
              ? cursorTouches(state, parent.from, parent.to)
              : active.has(state.doc.lineAt(nf).number);
          }
          if (revealed || isFence) {
            decos.push(Decoration.mark({ class: "cm-syntax" }).range(nf, nt));
          } else {
            let end = nt;
            if (blockMark && state.doc.sliceString(nt, nt + 1) === " ") end = nt + 1;
            decos.push(Decoration.replace({}).range(nf, end));
          }
        }
      },
    });
  }

  return Decoration.set(decos, true);
}

const livePreview = ViewPlugin.fromClass(
  class {
    constructor(view) { this.decorations = buildDecorations(view); }
    update(u) {
      if (u.docChanged || u.viewportChanged || u.selectionSet)
        this.decorations = buildDecorations(u.view);
    }
  },
  { decorations: (v) => v.decorations }
);

/* ================================================================== *
 *  Editor + file state
 * ================================================================== */

const searchbar = document.getElementById("searchbar");
const searchwrap = document.querySelector(".searchwrap");
const dirtyDot = document.getElementById("dirtyDot");
const resultsEl = document.getElementById("results");
const wordcount = document.getElementById("wordcount");
const confirmEl = document.getElementById("confirm");
const confirmOpts = document.getElementById("confirmOpts");

let currentPath = null;
let currentTitle = "";
let originalContent = "";
let loadedRaw = "";       // exact on-disk content (original wrapping)
let loadedReflowed = "";  // reflowed view of it

// Typewriter-scrolling hooks — the real centerCaret is assigned once the
// view exists (see smoothScroll below). scheduleCenter coalesces calls into
// one rAF so rapid cursor moves don't thrash.
let centerCaret = () => {};
let smearMove = () => {};   // Neovide-style cursor smear; real impl assigned below
let centerRaf = null;
function scheduleCenter() {
  if (centerRaf !== null) return;
  centerRaf = requestAnimationFrame(() => { centerRaf = null; centerCaret(true); });
}
// Hoisted here because the view's initial update fires updateRead()/scheduleFlash()
// during construction — a `let` declared after the editor would still be in its
// temporal dead zone and throw, blanking the whole UI.
let readRaf = null;
let flashRaf = null;
// Hoisted: the update listener reads findOpen during the view's initial
// construction update, so it must be initialized before the view is built.
let findOpen = false;
// Cursor look + smear, driven by the settings overlay (applySettings sets them).
// Hoisted for the same reason as findOpen — the update listener reads them
// during the view's initial construction update.
let cursorBlock = false;
let smearEnabled = true;
let cursorBlockW = 0;       // measured glyph width under the caret (block mode)

const view = new EditorView({
  parent: document.getElementById("editor"),
  state: EditorState.create({
    doc: "",
    extensions: [
      history(),
      drawSelection(),
      EditorView.lineWrapping,
      markdown({ base: markdownLanguage, addKeymap: true }),
      rawTablesField,
      tableField,
      livePreview,
      placeholder("Press ⌘K to open a .jd file, or just start writing…"),
      keymap.of([
        { key: "Mod-s", preventDefault: true, run: () => { saveFile(); return true; } },
        // swallow CM's default Ctrl-k (deleteLine) so the global search shortcut
        // wins on Windows/Linux; the window handler below does the focus.
        { key: "Mod-k", preventDefault: true, run: () => true },
        // Alt/Option + ←/→ jump by word (Shift extends the selection)
        { key: "Alt-ArrowLeft", run: cursorGroupLeft, shift: selectGroupLeft, preventDefault: true },
        { key: "Alt-ArrowRight", run: cursorGroupRight, shift: selectGroupRight, preventDefault: true },
        ...tableNavKeymap,
        ...defaultKeymap,
        ...historyKeymap,
      ]),
      // Cmd/Ctrl+click a link → follow it (.jd opens here, URLs open a tab)
      EditorView.domEventHandlers({
        mousedown(e) {
          if (!(e.metaKey || e.ctrlKey)) return false;
          const el = e.target.closest && e.target.closest("[data-href]");
          if (!el) return false;
          e.preventDefault();
          followLink(el.getAttribute("data-href"));
          return true;
        },
      }),
      EditorView.updateListener.of((u) => {
        if (u.selectionSet || u.docChanged) { scheduleCenter(); scheduleFlash(); smearMove(); updateCursorBlockWidth(); }
        if (u.docChanged || u.geometryChanged) updateRead();
        if (findOpen && u.docChanged) updateFindCount();   // field recomputed; sync the n/m counter
        if (!u.docChanged) return;
        markDirty();
        updateWordCount();
        // clearing a file's whole content → offer to delete the file
        if (currentPath && confirmEl.hidden &&
            originalContent.trim() !== "" && getContent().trim() === "") {
          showConfirm();
        } else if (!confirmEl.hidden && getContent().trim() !== "") {
          hideConfirm();
        }
      }),
    ],
  }),
});

/* ------------------------------------------------------------------ *
 *  Typewriter scrolling + inertia. A single rAF loop eases scrollTop
 *  toward `target`. The mouse wheel feeds momentum into it (buttery,
 *  weighted scroll); moving the cursor recenters `target` on the caret's
 *  line, so the active line stays vertically pinned to the middle of the
 *  screen while the text glides up and down underneath it — the iA Writer
 *  typewriter feel. The half-page top/bottom padding lets the very first
 *  and last lines reach the center too. Disabled under reduced-motion.
 * ------------------------------------------------------------------ */
(function smoothScroll() {
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const scroller = view.scrollDOM;
  let target = scroller.scrollTop;
  let raf = null;
  const max = () => scroller.scrollHeight - scroller.clientHeight;
  const clamp = (v) => Math.max(0, Math.min(max(), v));
  function tick() {
    const cur = scroller.scrollTop;
    const next = cur + (target - cur) * 0.18;         // lerp → momentum glide
    if (Math.abs(target - next) < 0.4) { scroller.scrollTop = target; raf = null; return; }
    scroller.scrollTop = next;
    raf = requestAnimationFrame(tick);
  }
  function glideTo(t) { target = clamp(t); if (raf === null) raf = requestAnimationFrame(tick); }

  // wheel → momentum (also lets you scroll away from the centered line to read)
  if (!reduce) {
    scroller.addEventListener("wheel", (e) => {
      if (e.ctrlKey) return;                          // pinch-zoom → leave it
      let dy = e.deltaY;
      if (e.deltaMode === 1) dy *= 18;                // line units → px
      else if (e.deltaMode === 2) dy *= scroller.clientHeight;
      e.preventDefault();
      if (raf === null) target = scroller.scrollTop;  // re-sync when idle
      glideTo(target + dy);
    }, { passive: false });
  }

  // typewriter: keep the caret's line vertically centered on cursor moves
  centerCaret = (animated) => {
    if (!view.state.selection.main.empty) {
      // A range selection is active — don't recenter. Crucially, also abort any
      // in-flight momentum glide (e.g. the recenter kicked off by the initial
      // click) and resync the target, so the typewriter loop stops fighting the
      // drag and native edge-autoscroll can extend the selection freely.
      if (raf !== null) { cancelAnimationFrame(raf); raf = null; }
      target = scroller.scrollTop;
      return;
    }
    const head = view.state.selection.main.head;
    const coords = view.coordsAtPos(head);
    if (!coords) {                                    // off-screen → bring it in first
      view.dispatch({ effects: EditorView.scrollIntoView(head, { y: "center" }) });
      return;
    }
    const rect = scroller.getBoundingClientRect();
    const caretY = (coords.top + coords.bottom) / 2;
    const centerY = rect.top + scroller.clientHeight / 2;
    const t = clamp(scroller.scrollTop + (caretY - centerY));
    if (animated && !reduce) glideTo(t);
    else { scroller.scrollTop = t; target = t; }
  };
  centerCaret(false);                                 // center whatever's loaded now
})();

/* ------------------------------------------------------------------ *
 *  Neovide-style cursor smear. A tinted shape stretches between the
 *  caret's previous and current positions and contracts into the caret
 *  as it catches up, so the eye can always track where the cursor went.
 *  Implemented as the convex hull of the old + new caret rectangles,
 *  painted via clip-path on a fixed, viewport-covering layer (a single
 *  clip-path update per frame — no layout, GPU-composited). The target
 *  is re-read every frame so the smear rides typewriter scrolling, and
 *  long jumps (e.g. opening a file) snap instead of streaking the screen.
 * ------------------------------------------------------------------ */
(function smearCursor() {
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const layer = document.createElement("div");
  layer.className = "cm-smear-layer";
  const smear = document.createElement("div");
  smear.className = "cm-smear";
  layer.appendChild(smear);
  document.body.appendChild(layer);

  let from = null, to = null, t = 0, step = 0.1, raf = null;

  // Caret rect in scroll-invariant CONTENT coords, so a captured from/to keeps
  // tracking the text while the typewriter scroll glides underneath it.
  const caretRect = () => {
    const sel = view.state.selection.main;
    if (!sel.empty) return null;                 // only smear a collapsed caret
    const c = view.coordsAtPos(sel.head);
    if (!c) return null;
    const b = view.scrollDOM.getBoundingClientRect();
    return {
      x: c.left - b.left + view.scrollDOM.scrollLeft,
      y: c.top - b.top + view.scrollDOM.scrollTop,
      w: cursorBlock ? Math.max(cursorBlockW || view.defaultCharacterWidth, 3) : 2,
      h: c.bottom - c.top,
    };
  };
  // Project a content-coord rect back into viewport space for painting.
  const toView = (r) => {
    const b = view.scrollDOM.getBoundingClientRect();
    return {
      x: r.x + b.left - view.scrollDOM.scrollLeft,
      y: r.y + b.top - view.scrollDOM.scrollTop,
      w: r.w, h: r.h,
    };
  };
  const lerp = (a, b, k) => ({
    x: a.x + (b.x - a.x) * k, y: a.y + (b.y - a.y) * k,
    w: a.w + (b.w - a.w) * k, h: a.h + (b.h - a.h) * k,
  });
  const corners = (r) => [[r.x, r.y], [r.x + r.w, r.y], [r.x + r.w, r.y + r.h], [r.x, r.y + r.h]];
  // Andrew's monotone-chain convex hull of the 8 corner points.
  const hull = (pts) => {
    pts = pts.slice().sort((p, q) => p[0] - q[0] || p[1] - q[1]);
    const cross = (o, a, b) => (a[0]-o[0])*(b[1]-o[1]) - (a[1]-o[1])*(b[0]-o[0]);
    const lo = [], up = [];
    for (const p of pts) { while (lo.length >= 2 && cross(lo[lo.length-2], lo[lo.length-1], p) <= 0) lo.pop(); lo.push(p); }
    for (let i = pts.length - 1; i >= 0; i--) { const p = pts[i]; while (up.length >= 2 && cross(up[up.length-2], up[up.length-1], p) <= 0) up.pop(); up.push(p); }
    lo.pop(); up.pop();
    return lo.concat(up);
  };
  const draw = (head, tail) => {
    const pts = hull([...corners(toView(head)), ...corners(toView(tail))]);
    if (pts.length < 3) return;
    smear.style.clipPath = "polygon(" + pts.map((p) => `${p[0]}px ${p[1]}px`).join(",") + ")";
  };

  const ease = (x) => 1 - Math.pow(1 - x, 3);     // easeOutCubic
  function tick() {
    raf = null;
    if (!from || !to) { layer.classList.remove("on"); return; }
    t = Math.min(1, t + step);
    // leading edge reaches the target by t=0.35; trailing edge stays pinned at
    // the origin until t=0.5, then catches up — a full-length streak that holds
    // a beat, then collapses into the caret.
    const head = lerp(from, to, ease(Math.min(1, t / 0.35)));
    const tail = lerp(from, to, ease(Math.max(0, (t - 0.5) / 0.5)));
    draw(head, tail);
    if (t >= 1) { layer.classList.remove("on"); return; }
    layer.classList.add("on");
    raf = requestAnimationFrame(tick);
  }

  smearMove = () => {
    const r = caretRect();
    if (!r || !smearEnabled) { layer.classList.remove("on"); to = r; if (raf) { cancelAnimationFrame(raf); raf = null; } return; }
    const jump = to ? Math.hypot(r.x - to.x, r.y - to.y) : 0;
    if (!to || jump < 0.5) { to = r; return; }     // first paint / no real move → just snap
    from = to;                                     // streak starts where the caret just was
    to = r;
    t = 0;
    // Trail rides the distance: the streak spans the full from→to gap (so a
    // cross-screen jump streaks across the screen), and a longer jump also rides
    // through more frames so it lives — and reads — longer. Roughly constant
    // travel speed (~PX_PER_FRAME) between clamped min/max durations.
    const PX_PER_FRAME = 55, MIN_FRAMES = 4, MAX_FRAMES = 30;
    const frames = Math.max(MIN_FRAMES, Math.min(MAX_FRAMES, Math.round(jump / PX_PER_FRAME)));
    step = 1 / frames;
    if (raf === null) raf = requestAnimationFrame(tick);
  };
})();

/* Block cursor: when enabled, the native caret is widened into a block sized to
   the glyph under it — measured live, since the prose font is proportional. The
   smear above reads the same width, so a moving block streaks like Neovide. */
function measureGlyphWidth(pos) {
  const a = view.coordsAtPos(pos);
  const b = view.coordsAtPos(pos + 1);
  if (a && b && Math.abs(b.top - a.top) < 1 && b.left > a.left) return b.left - a.left;
  return view.defaultCharacterWidth;
}
function updateCursorBlockWidth() {
  if (!cursorBlock) return;
  cursorBlockW = measureGlyphWidth(view.state.selection.main.head);
  view.dom.style.setProperty("--cursor-w", `${cursorBlockW}px`);
}

/* Reading progress — fill the searchbar's bottom border (--read, 0–1) by how
   far you've scrolled through the document. Coalesced into a rAF off scroll. */
function updateRead() {
  if (readRaf !== null) return;
  readRaf = requestAnimationFrame(() => {
    readRaf = null;
    const s = view.scrollDOM;
    const max = s.scrollHeight - s.clientHeight;
    searchwrap.style.setProperty("--read", max > 0 ? (s.scrollTop / max).toFixed(4) : "0");
  });
}
view.scrollDOM.addEventListener("scroll", updateRead, { passive: true });
new ResizeObserver(updateRead).observe(view.scrollDOM);
updateRead();

// Retrigger the caret glow flash on every move/keystroke. CM reuses the caret
// element, so restart the CSS animation by reflowing between class toggles.
// Deferred a frame so the caret is drawn at its new position first.
function scheduleFlash() {
  if (flashRaf !== null) return;
  flashRaf = requestAnimationFrame(() => { flashRaf = null; flashCaret(); });
}
function flashCaret() {
  const caret = view.scrollDOM.querySelector(".cm-cursor-primary");
  if (!caret) return;
  caret.classList.remove("cm-flash");
  void caret.offsetWidth;          // force reflow → animation replays from 0%
  caret.classList.add("cm-flash");
}

const getContent = () => view.state.doc.toString();
function setContent(text) {
  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
}
function markDirty() {
  dirtyDot.classList.toggle("show", getContent() !== originalContent);
}

// live word count + reading time of the body (frontmatter excluded)
function updateWordCount() {
  const doc = view.state.doc;
  const fm = frontmatter(view.state);
  let from = 0;
  if (fm) from = fm.close < doc.lines ? doc.line(fm.close + 1).from : doc.length;
  const words = (doc.sliceString(from).match(/\S+/g) || []).length;
  const label = words === 1 ? "1 word" : `${words} words`;
  // ~200 wpm; only show reading time once it's meaningful
  wordcount.textContent = words >= 100 ? `${label} · ${Math.round(words / 200)} min` : label;
}

// show the current file's title in the searchbar (unless mid-search)
function showTitle() {
  if (!searching) searchbar.value = currentTitle;
}

// Unwrap hard-wrapped prose so paragraphs flow and re-wrap to the column,
// like normal text. Structure (frontmatter, headings, lists, quotes, tables,
// code fences, blank lines) is preserved verbatim — only plain prose lines
// in a paragraph are joined.
function reflow(text) {
  const lines = text.split("\n");
  const out = [];
  let i = 0;
  if (lines[0] !== undefined && lines[0].trim() === "---") {
    out.push(lines[i++]);
    while (i < lines.length && lines[i].trim() !== "---") out.push(lines[i++]);
    if (i < lines.length) out.push(lines[i++]); // closing ---
  }
  let inFence = false;
  let para = [];
  const flush = () => { if (para.length) { out.push(para.join(" ")); para = []; } };
  for (; i < lines.length; i++) {
    const line = lines[i];
    const t = line.trim();
    if (/^(```|~~~)/.test(t)) { flush(); inFence = !inFence; out.push(line); continue; }
    if (inFence) { out.push(line); continue; }
    const structural =
      t === "" ||                       // blank line (paragraph break)
      /^#{1,6}\s/.test(t) ||            // heading
      /^([-*+]|\d+[.)])\s/.test(t) ||  // list item
      /^>/.test(t) ||                   // blockquote
      /^\|.*\|/.test(t) ||             // table row
      /^([-*_])\1{2,}\s*$/.test(t) ||  // thematic break
      /^(\s{4,}|\t)/.test(line);        // indented code
    if (structural) { flush(); out.push(line); continue; }
    para.push(t); // plain prose → accumulate into the current paragraph
  }
  flush();
  return out.join("\n");
}

// follow a markdown link: http(s) → new tab; otherwise resolve relative to
// the current file's directory and open the .jd here.
function followLink(href) {
  if (!href) return;
  if (/^https?:\/\//i.test(href)) {
    window.open(href, "_blank", "noopener");
    return;
  }
  const stack = currentPath ? currentPath.split("/").slice(0, -1) : [];
  for (const part of href.split("/")) {
    if (part === "..") stack.pop();
    else if (part && part !== ".") stack.push(part);
  }
  openFile(stack.join("/"));
}

async function openFile(p) {
  try {
    const res = await fetch(`/api/file?path=${encodeURIComponent(p)}`);
    const data = await res.json();
    if (data.error) return false;
    currentPath = data.path;
    currentTitle = data.path.split("/").pop();
    const flowed = reflow(data.content);
    loadedRaw = data.content;   // keep the exact on-disk text
    loadedReflowed = flowed;
    originalContent = flowed;   // track edits against the reflowed text (no false-dirty)
    setContent(flowed);
    // restart the gentle content-entrance animation on each open
    const host = document.getElementById("editor");
    host.classList.remove("just-loaded");
    void host.offsetWidth;
    host.classList.add("just-loaded");
    showTitle();
    updateWordCount();
    dirtyDot.classList.remove("show");
    try { localStorage.setItem("jd:last", data.path); } catch {}
    // land the cursor in the body (just past any frontmatter)
    const fm = frontmatter(view.state);
    const startLine = fm ? Math.min(fm.close + 2, view.state.doc.lines) : 1;
    const pos = view.state.doc.line(startLine).from;
    view.dispatch({ selection: { anchor: pos } });   // centerCaret handles scroll
    view.focus();
    return true;
  } catch (err) {
    console.error("open failed:", err);
    return false;
  }
}

async function saveFile() {
  if (!currentPath) return;
  try {
    const content = getContent();
    // unedited → write the original wrapping back; edited → write the new text
    const toWrite = content === loadedReflowed ? loadedRaw : content;
    const res = await fetch(`/api/file?path=${encodeURIComponent(currentPath)}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content: toWrite }),
    });
    const data = await res.json();
    if (data.success) {
      originalContent = content;
      dirtyDot.classList.remove("show");
      flashSaved();
    }
  } catch (err) {
    console.error("save failed:", err);
  }
}

function flashSaved() {
  searchbar.classList.add("saved-flash");
  setTimeout(() => searchbar.classList.remove("saved-flash"), 600);
}

/* ================================================================== *
 *  Top searchbar — shows the file title; ⌘K / focus turns it into a
 *  ripgrep + fzf file search. Picking a result shows its title here.
 * ================================================================== */

let matches = [];
let total = 0;
let selected = 0;
let searchTimer = null;
let searching = false;
let staggerNext = false;        // stagger result entrance only on open
let lastQuery = "";             // for highlighting matches in results
let searchSeq = 0;              // guards against out-of-order responses
let canCreate = false;          // whether the "create new file" row is shown

function enterSearch() {
  if (searching) return;
  searching = true;
  searchbar.value = "";
  searchwrap.classList.add("searching");
  staggerNext = true;
  runSearch("");
}

function exitSearch() {
  if (!searching) return;
  searching = false;
  searchwrap.classList.remove("searching");
  // subtle fade-out, then hide — only commit the hide if still closed (a fast
  // reopen within the window clears .closing in renderResults and aborts this).
  resultsEl.classList.remove("stagger");
  resultsEl.classList.add("closing");
  setTimeout(() => { if (!searching) resultsEl.hidden = true; }, 160);
  showTitle();
}

async function runSearch(q) {
  const seq = ++searchSeq;
  try {
    const res = await fetch(`/api/search?q=${encodeURIComponent(q)}`);
    const data = await res.json();
    if (seq !== searchSeq) return;   // a newer query already superseded this one
    matches = data.results || [];
    total = data.total || matches.length;
    lastQuery = q;
    selected = 0;
    renderResults();
  } catch (err) {
    if (seq !== searchSeq) return;
    resultsEl.hidden = false;
    resultsEl.innerHTML = '<div class="result-empty">search unavailable</div>';
  }
}

function renderResults() {
  const stagger = staggerNext;
  staggerNext = false;
  resultsEl.classList.remove("closing");   // cancel any in-flight fade-out
  resultsEl.hidden = false;
  // offer to create a new .jd when the query names a file that doesn't exist
  const q = lastQuery.trim();
  const wanted = q.replace(/\.jd$/i, "").toLowerCase() + ".jd";
  canCreate = q.length > 0 && /^[\w .\-/]+$/.test(q) &&
              !matches.some((r) => r.name.toLowerCase() === wanted);

  if (!matches.length && !canCreate) {
    resultsEl.classList.remove("stagger");
    resultsEl.innerHTML = '<div class="result-empty">no matches</div>';
    return;
  }
  resultsEl.classList.toggle("stagger", stagger);
  let html = matches.map((r, i) => `
    <div class="result-item${i === selected ? " active" : ""}" data-i="${i}">
      <div class="pi-main">
        <span class="pi-name">${highlight(r.name, lastQuery)}</span>
        <span class="pi-dir">${highlight(r.dir || "", lastQuery)}</span>
      </div>
      ${r.snippet ? `<div class="pi-snip">${highlight(r.snippet, lastQuery)}</div>` : ""}
    </div>`).join("");
  if (canCreate) {
    const ci = matches.length;
    const fname = q.replace(/\.jd$/i, "") + ".jd";
    const dir = currentPath ? currentPath.split("/").slice(0, -1).join("/") : "";
    html += `
      <div class="result-item create${ci === selected ? " active" : ""}" data-i="${ci}">
        <div class="pi-main">
          <span class="pi-name">＋ Create “${escapeHtml(fname)}”</span>
          <span class="pi-dir">${escapeHtml(dir || "(root)")}</span>
        </div>
      </div>`;
  }
  resultsEl.innerHTML = html;
  if (total > matches.length) {
    resultsEl.insertAdjacentHTML("beforeend",
      `<div class="result-foot">${matches.length} of ${total}</div>`);
  }
  resultsEl.querySelectorAll(".result-item").forEach((el) => {
    el.onmousemove = () => { selected = +el.dataset.i; paintSelection(); };
    // mousedown (not click) so it fires before the input's blur
    el.onmousedown = (e) => { e.preventDefault(); choose(+el.dataset.i); };
  });
}

function paintSelection() {
  resultsEl.querySelectorAll(".result-item").forEach((el, i) =>
    el.classList.toggle("active", i === selected));
}

function choose(i) {
  if (canCreate && i === matches.length) { createFile(lastQuery); return; }
  const r = matches[i];
  if (!r) return;
  openFile(r.path);   // sets currentTitle + showTitle once loaded
  exitSearch();
}

// create a new .jd (sibling of the current file) with a starter template
async function createFile(name) {
  const base = name.trim().replace(/\.jd$/i, "");
  if (!base) return;
  const dir = currentPath ? currentPath.split("/").slice(0, -1).join("/") : "";
  const path = (dir ? dir + "/" : "") + base + ".jd";
  const template = `---\nname: ${base}\nkind: tool\ndescription: \n---\n\n# ${base}\n\n`;
  try {
    await fetch(`/api/file?path=${encodeURIComponent(path)}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content: template }),
    });
  } catch {}
  exitSearch();
  openFile(path);
}

function scrollToSelected() {
  const el = resultsEl.querySelector(".result-item.active");
  if (el) el.scrollIntoView({ block: "nearest" });
}

searchbar.addEventListener("focus", enterSearch);
searchbar.addEventListener("blur", exitSearch);

searchbar.addEventListener("input", () => {
  if (!searching) return;
  clearTimeout(searchTimer);
  const q = searchbar.value;
  searchTimer = setTimeout(() => runSearch(q), 80);
});

searchbar.addEventListener("keydown", (e) => {
  if (!searching) return;
  const maxIdx = matches.length - 1 + (canCreate ? 1 : 0);
  if (e.key === "ArrowDown") { e.preventDefault(); selected = Math.min(selected + 1, maxIdx); paintSelection(); scrollToSelected(); }
  else if (e.key === "ArrowUp") { e.preventDefault(); selected = Math.max(selected - 1, 0); paintSelection(); scrollToSelected(); }
  else if (e.key === "Enter") { e.preventDefault(); choose(selected); }
  else if (e.key === "Escape") { e.preventDefault(); searchbar.blur(); }
});

// Global ⌘K (search) and ⌘O (reveal the file's folder in Finder).
window.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === "k") {
    e.preventDefault();
    if (searching) searchbar.blur();
    else searchbar.focus();
  } else if ((e.metaKey || e.ctrlKey) && e.key === "o") {
    e.preventDefault();
    if (currentPath) fetch(`/api/reveal?path=${encodeURIComponent(currentPath)}`, { method: "POST" });
  } else if ((e.metaKey || e.ctrlKey) && (e.key === "f" || e.key === "F")) {
    // ⌃F find · ⌃⇧F find + replace
    e.preventDefault();
    openFind(e.shiftKey);
  } else if ((e.metaKey || e.ctrlKey) && (e.key === "g" || e.key === "G")) {
    // ⌃G global ripgrep palette
    e.preventDefault();
    rgOpen ? closeRg(false) : openRg();
  } else if ((e.metaKey || e.ctrlKey) && e.key === ",") {
    // ⌃, settings (accent + fade)
    e.preventDefault();
    settingsOpen ? closeSettings() : openSettings();
  } else if (e.key === "Escape" && settingsOpen) {
    e.preventDefault();
    closeSettings();
  }
});

/* ---- delete-file confirm: y/n, ←/→ · h/l to move, Enter to choose ---- */
let confirmChoice = "n";
function renderConfirm() {
  confirmOpts.innerHTML =
    `<span class="copt${confirmChoice === "y" ? " sel" : ""}">y</span>` +
    `<span class="cslash">/</span>` +
    `<span class="copt${confirmChoice === "n" ? " sel" : ""}">n</span>`;
}
function showConfirm() {
  confirmChoice = "n";
  renderConfirm();
  confirmEl.hidden = false;
  requestAnimationFrame(() => confirmEl.classList.add("open"));
}
function hideConfirm() {
  confirmEl.classList.remove("open");
  setTimeout(() => { confirmEl.hidden = true; }, 160);
  view.focus();
}
async function deleteCurrentFile() {
  const gone = currentPath;
  if (!gone) { hideConfirm(); return; }
  try { await fetch(`/api/delete?path=${encodeURIComponent(gone)}`, { method: "POST" }); } catch {}
  hideConfirm();
  currentPath = null; currentTitle = ""; originalContent = "";
  searchbar.value = ""; dirtyDot.classList.remove("show");
  try {
    const res = await fetch("/api/search?q=");
    const data = await res.json();
    const next = (data.results || []).find((r) => r.path !== gone);
    if (next) openFile(next.path);
  } catch {}
}

// capture-phase so the confirm owns the keyboard before the editor sees it
window.addEventListener("keydown", (e) => {
  if (confirmEl.hidden) return;
  const k = e.key;
  if (k === "ArrowLeft" || k === "h") { confirmChoice = "y"; renderConfirm(); }
  else if (k === "ArrowRight" || k === "l") { confirmChoice = "n"; renderConfirm(); }
  else if (k === "y" || k === "Y") { deleteCurrentFile(); }
  else if (k === "Escape" || k === "n" || k === "N") { hideConfirm(); }
  else if (k === "Enter") { confirmChoice === "y" ? deleteCurrentFile() : hideConfirm(); }
  e.preventDefault();
  e.stopPropagation();
}, true);

/* ================================================================== *
 *  Local find / replace (⌃F · ⌃⇧F) + global ripgrep (⌃G).
 *  Matching uses @codemirror/search's SearchCursor; the match list lives
 *  in a StateField so it recomputes on doc changes *inside* the field
 *  (no dispatch from the update listener, which CM forbids). ripgrep runs
 *  server-side (/api/rg) — case-sensitive, literal, .jd only — and its hits
 *  preview live in the editor as you arrow through the dropdown.
 * ================================================================== */

const findBar = document.getElementById("findbar");
const findInput = document.getElementById("findInput");
const findReplaceInput = document.getElementById("findReplace");
const findCount = document.getElementById("findCount");
const findCaseBtn = document.getElementById("findCase");

let findCase = false;        // case-sensitive toggle (off = case-insensitive)
let findFieldAdded = false;

const setFindQuery = StateEffect.define();   // { query, cs } — resets the active match
const setFindIndex = StateEffect.define();   // number — moves the active match
const findDefault = { query: "", cs: false, matches: [], index: -1, deco: Decoration.none };

function pickIndex(matches, head) {
  if (!matches.length) return -1;
  const at = matches.findIndex((m) => m.from >= head);
  return at === -1 ? 0 : at;
}

const findField = StateField.define({
  create() { return findDefault; },
  update(val, tr) {
    let { query, cs, index } = val;
    let queryChanged = false, indexSet = null;
    for (const e of tr.effects) {
      if (e.is(setFindQuery)) { query = e.value.query; cs = e.value.cs; queryChanged = true; }
      else if (e.is(setFindIndex)) indexSet = e.value;
    }
    if (!tr.docChanged && !queryChanged && indexSet === null) return val;
    let matches = val.matches;
    if (tr.docChanged || queryChanged) {
      matches = [];
      if (query) {
        const len = tr.state.doc.length;
        const cur = cs
          ? new SearchCursor(tr.state.doc, query, 0, len)
          : new SearchCursor(tr.state.doc, query, 0, len, (s) => s.toLowerCase());
        while (!cur.next().done) matches.push({ from: cur.value.from, to: cur.value.to });
      }
    }
    if (indexSet !== null) index = indexSet;
    else if (queryChanged || tr.docChanged) index = pickIndex(matches, tr.state.selection.main.head);
    if (index >= matches.length) index = matches.length ? 0 : -1;
    const deco = matches.length
      ? Decoration.set(matches.map((m, i) =>
          Decoration.mark({ class: i === index ? "cm-find-match cm-find-current" : "cm-find-match" })
            .range(m.from, m.to)), true)
      : Decoration.none;
    return { query, cs, matches, index, deco };
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.deco),
});

const findState = () => view.state.field(findField, false) || findDefault;

function updateFindCount() {
  const fs = findState();
  if (!findInput.value) findCount.textContent = "";
  else if (!fs.matches.length) findCount.textContent = "0/0";
  else findCount.textContent = `${fs.index + 1}/${fs.matches.length}`;
}

// Move the editor selection onto the active match and center it.
function revealCurrent() {
  const fs = findState();
  const m = fs.matches[fs.index];
  if (!m) return;
  view.dispatch({
    selection: { anchor: m.from, head: m.to },
    effects: EditorView.scrollIntoView(m.from, { y: "center" }),
  });
}

function applyFindQuery(reveal) {
  view.dispatch({ effects: setFindQuery.of({ query: findInput.value, cs: findCase }) });
  updateFindCount();
  if (reveal) revealCurrent();
}

function gotoMatch(delta) {
  const fs = findState();
  if (!fs.matches.length) return;
  const idx = (fs.index + delta + fs.matches.length) % fs.matches.length;
  const m = fs.matches[idx];
  view.dispatch({
    selection: { anchor: m.from, head: m.to },
    effects: [setFindIndex.of(idx), EditorView.scrollIntoView(m.from, { y: "center" })],
  });
  updateFindCount();
}

function replaceCurrent() {
  const fs = findState();
  const m = fs.matches[fs.index];
  if (!m) return;
  view.dispatch({ changes: { from: m.from, to: m.to, insert: findReplaceInput.value } });
  // the field recomputed matches + repicked the index (after the insert)
  updateFindCount();
  revealCurrent();
}

function replaceAll() {
  const fs = findState();
  if (!fs.matches.length) return;
  const rep = findReplaceInput.value;
  view.dispatch({ changes: fs.matches.map((m) => ({ from: m.from, to: m.to, insert: rep })) });
  updateFindCount();
}

function openFind(replace) {
  if (!findFieldAdded) {
    view.dispatch({ effects: StateEffect.appendConfig.of([findField]) });
    findFieldAdded = true;
  }
  findOpen = true;
  findBar.hidden = false;
  findBar.classList.toggle("with-replace", !!replace);
  requestAnimationFrame(() => findBar.classList.add("open"));
  const sel = view.state.selection.main;
  if (!sel.empty) findInput.value = view.state.sliceDoc(sel.from, sel.to);
  findInput.focus();
  findInput.select();
  applyFindQuery(true);
}

function closeFind() {
  if (!findOpen) return;
  findOpen = false;
  findBar.classList.remove("open");
  setTimeout(() => { findBar.hidden = true; }, 160);
  if (findFieldAdded) view.dispatch({ effects: setFindQuery.of({ query: "", cs: findCase }) }); // clear highlights
  view.focus();
}

findInput.addEventListener("input", () => applyFindQuery(true));
findInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") { e.preventDefault(); gotoMatch(e.shiftKey ? -1 : 1); }
  else if (e.key === "Escape") { e.preventDefault(); closeFind(); }
});
findReplaceInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") { e.preventDefault(); e.shiftKey ? replaceAll() : replaceCurrent(); }
  else if (e.key === "Escape") { e.preventDefault(); closeFind(); }
});
findCaseBtn.addEventListener("click", () => {
  findCase = !findCase;
  findCaseBtn.classList.toggle("on", findCase);
  applyFindQuery(true);
  findInput.focus();
});
document.getElementById("findPrev").addEventListener("click", () => { gotoMatch(-1); findInput.focus(); });
document.getElementById("findNext").addEventListener("click", () => { gotoMatch(1); findInput.focus(); });
document.getElementById("findClose").addEventListener("click", () => closeFind());
document.getElementById("findReplaceOne").addEventListener("click", () => { replaceCurrent(); findReplaceInput.focus(); });
document.getElementById("findReplaceAll").addEventListener("click", () => { replaceAll(); findReplaceInput.focus(); });

/* ---- global ripgrep palette (⌃G) ---- */
const rgPanel = document.getElementById("rgpanel");
const rgInput = document.getElementById("rgInput");
const rgResults = document.getElementById("rgResults");

let rgItems = [];
let rgSel = 0;
let rgOpen = false;
let rgError = null;
let rgTimer = null;
let rgSeq = 0;
let rgPreviewSeq = 0;
let rgReturn = null;   // {path, anchor} restored when the palette is cancelled

function openRg() {
  if (rgOpen) return;
  rgOpen = true;
  rgReturn = currentPath ? { path: currentPath, anchor: view.state.selection.main.head } : null;
  rgPanel.hidden = false;
  requestAnimationFrame(() => rgPanel.classList.add("open"));
  rgInput.value = "";
  rgItems = []; rgSel = 0; rgError = null;
  rgResults.innerHTML = "";
  rgInput.focus();
}

function closeRg(cancel) {
  if (!rgOpen) return;
  rgOpen = false;
  rgPanel.classList.remove("open");
  setTimeout(() => { rgPanel.hidden = true; }, 160);
  if (cancel && rgReturn && rgReturn.path !== currentPath) {
    const { path, anchor } = rgReturn;
    openFile(path).then(() => {
      view.dispatch({ selection: { anchor }, effects: EditorView.scrollIntoView(anchor, { y: "center" }) });
      view.focus();
    });
  } else {
    view.focus();
  }
  rgReturn = null;
}

async function runRg(q) {
  const seq = ++rgSeq;
  if (!q.trim()) { rgItems = []; rgError = null; renderRg(); return; }
  try {
    const res = await fetch(`/api/rg?q=${encodeURIComponent(q)}`);
    const data = await res.json();
    if (seq !== rgSeq) return;
    rgItems = data.results || [];
    rgError = data.error || null;
    rgSel = 0;
    renderRg();
    if (rgItems.length) previewRg(rgItems[0]);
  } catch {
    if (seq !== rgSeq) return;
    rgItems = []; rgError = "search unavailable"; renderRg();
  }
}

function renderRg() {
  if (rgError && !rgItems.length) {
    rgResults.innerHTML = `<div class="result-empty">${escapeHtml(rgError)}</div>`;
    return;
  }
  if (!rgItems.length) {
    rgResults.innerHTML = rgInput.value.trim() ? '<div class="result-empty">no matches</div>' : "";
    return;
  }
  rgResults.innerHTML = rgItems.map((r, i) => `
    <div class="rg-item${i === rgSel ? " active" : ""}" data-i="${i}">
      <div class="rg-head">
        <span class="rg-name">${escapeHtml(r.name)}</span>
        <span class="rg-loc">${escapeHtml(r.dir || "")}:${r.line}</span>
      </div>
      <div class="rg-snip">${highlight(r.text, rgInput.value)}</div>
    </div>`).join("");
  rgResults.querySelectorAll(".rg-item").forEach((el) => {
    el.onmousemove = () => { rgSel = +el.dataset.i; paintRg(); previewRg(rgItems[rgSel]); };
    el.onmousedown = (e) => { e.preventDefault(); rgSel = +el.dataset.i; confirmRg(); };
  });
  scrollRgIntoView();
}

function paintRg() {
  rgResults.querySelectorAll(".rg-item").forEach((el, i) => el.classList.toggle("active", i === rgSel));
}
function scrollRgIntoView() {
  const el = rgResults.querySelector(".rg-item.active");
  if (el) el.scrollIntoView({ block: "nearest" });
}

function moveRg(delta) {
  if (!rgItems.length) return;
  rgSel = (rgSel + delta + rgItems.length) % rgItems.length;
  paintRg();
  scrollRgIntoView();
  previewRg(rgItems[rgSel]);
}

// Show the highlighted hit in the editor (the "content area"): open its file
// if needed, then select the matched text on that line and center it.
async function previewRg(item) {
  if (!item) return;
  const seq = ++rgPreviewSeq;
  if (item.path !== currentPath) {
    const ok = await openFile(item.path);
    if (!ok || seq !== rgPreviewSeq) return;
  }
  const ln = Math.min(Math.max(item.line || 1, 1), view.state.doc.lines);
  const line = view.state.doc.line(ln);
  let from = line.from, to = line.from;
  const q = rgInput.value;
  if (q) {
    const cur = new SearchCursor(view.state.doc, q, line.from, line.to);   // rg is case-sensitive
    if (!cur.next().done) { from = cur.value.from; to = cur.value.to; }
  }
  view.dispatch({ selection: { anchor: from, head: to }, effects: EditorView.scrollIntoView(from, { y: "center" }) });
  if (rgOpen) rgInput.focus();   // keep the palette driving
}

function confirmRg() {
  rgReturn = null;          // keep the previewed hit; don't restore
  closeRg(false);
}

rgInput.addEventListener("input", () => {
  clearTimeout(rgTimer);
  const q = rgInput.value;
  rgTimer = setTimeout(() => runRg(q), 90);
});
rgInput.addEventListener("keydown", (e) => {
  if (e.key === "ArrowDown" || (e.ctrlKey && e.key === "j")) { e.preventDefault(); moveRg(1); }
  else if (e.key === "ArrowUp" || (e.ctrlKey && e.key === "k")) { e.preventDefault(); moveRg(-1); }
  else if (e.key === "Enter") { e.preventDefault(); confirmRg(); }
  else if (e.key === "Escape") { e.preventDefault(); closeRg(true); }
});

/* ================================================================== *
 *  Settings overlay (⌃,) — accent color + focus/edge fade tuning.
 *  Each control writes a CSS custom property on :root (inline, so it
 *  beats the stylesheet incl. the dark-mode block) and persists to
 *  localStorage. Empty value = fall back to the stylesheet default.
 * ================================================================== */

const settingsPanel = document.getElementById("settings");
const setTint = document.getElementById("setTint");
const setDim = document.getElementById("setDim");
const setFade = document.getElementById("setFade");
const setFont = document.getElementById("setFont");
const setMono = document.getElementById("setMono");
const settingsSwatches = document.getElementById("settingsSwatches");
const setCursor = document.getElementById("setCursor");
const setSmear = document.getElementById("setSmear");

// Curated Google Fonts. "" = the bundled iA Writer default (no network load).
// Fallback chains mirror the stylesheet's --font / --font-mono.
const FONT_FALLBACK = 'ui-rounded, "Helvetica Neue", -apple-system, sans-serif';
const MONO_FALLBACK = 'ui-monospace, "SF Mono", Menlo, monospace';
const BODY_FONTS = ["", "Inter", "Work Sans", "Lora", "Source Serif 4", "Newsreader",
                    "Spectral", "Literata", "EB Garamond"];
const MONO_FONTS = ["", "JetBrains Mono", "IBM Plex Mono", "Space Mono", "Roboto Mono", "Fira Code"];

// Inject (or update / remove) a Google Fonts <link> for the chosen family.
function loadGFont(id, name, italic) {
  let link = document.getElementById(id);
  if (!name) { if (link) link.remove(); return; }
  if (!link) {
    link = document.createElement("link");
    link.id = id; link.rel = "stylesheet";
    document.head.appendChild(link);
  }
  const fam = name.replace(/ /g, "+");
  const axis = italic ? ":ital,wght@0,400;0,700;1,400" : ":wght@400;700";
  link.href = `https://fonts.googleapis.com/css2?family=${fam}${axis}&display=swap`;
}

const SETTINGS_KEY = "jd:settings";
const SETTINGS_DEFAULTS = { tint: "", dim: "", fadeDist: "", font: "", mono: "", cursor: "block", smear: true };
const SWATCHES = ["#007aff", "#0a84ff", "#5e5ce6", "#34c759", "#ff9f0a", "#ff375f", "#bf5af2", "#1a1a1a"];
let settingsOpen = false;
let settings = loadSettings();

function loadSettings() {
  try { return { ...SETTINGS_DEFAULTS, ...(JSON.parse(localStorage.getItem(SETTINGS_KEY)) || {}) }; }
  catch { return { ...SETTINGS_DEFAULTS }; }
}
function saveSettings() {
  try { localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings)); } catch {}
}
function applySettings() {
  const root = document.documentElement.style;
  // --tint cascades into --selection / links automatically (they're var(--tint))
  settings.tint ? root.setProperty("--tint", settings.tint) : root.removeProperty("--tint");
  settings.dim ? root.setProperty("--dim", settings.dim) : root.removeProperty("--dim");
  settings.fadeDist ? root.setProperty("--fade-dist", settings.fadeDist) : root.removeProperty("--fade-dist");
  // fonts: load the webfont on demand, then point the CSS var at it (with the
  // stylesheet's fallback chain); empty → drop the link and the override.
  loadGFont("gfont-body", settings.font, true);
  loadGFont("gfont-mono", settings.mono, false);
  settings.font ? root.setProperty("--font", `"${settings.font}", ${FONT_FALLBACK}`) : root.removeProperty("--font");
  settings.mono ? root.setProperty("--font-mono", `"${settings.mono}", ${MONO_FALLBACK}`) : root.removeProperty("--font-mono");
  // preview: render each picker's own label in the family it selects (the body
  // picker in the body font, the mono picker in mono); empty → CSS default.
  setFont.style.fontFamily = settings.font ? `"${settings.font}", ${FONT_FALLBACK}` : "";
  setMono.style.fontFamily = settings.mono ? `"${settings.mono}", ${MONO_FALLBACK}` : "";
  // a swapped-in webfont changes glyph metrics — re-measure so the caret and
  // typewriter scroll stay aligned (CM caches character width otherwise).
  document.fonts?.ready.then(() => view.requestMeasure());
  // cursor look + smear
  cursorBlock = settings.cursor !== "line";
  smearEnabled = settings.smear !== false;
  view.dom.classList.toggle("cursor-block", cursorBlock);
  if (cursorBlock) updateCursorBlockWidth(); else view.dom.style.removeProperty("--cursor-w");
}

function hexOf(v) {
  v = (v || "").trim();
  return /^#[0-9a-fA-F]{6}$/.test(v) ? v.toLowerCase() : "#007aff";
}
function syncSettingsControls() {
  const cs = getComputedStyle(document.documentElement);
  setTint.value = hexOf(settings.tint || cs.getPropertyValue("--tint"));
  const dim = parseFloat(settings.dim || cs.getPropertyValue("--dim")) || 0.34;
  setDim.value = String(Math.round((1 - dim) * 100));            // higher slider = stronger fade
  const dist = parseFloat(settings.fadeDist || cs.getPropertyValue("--fade-dist") || "30");
  setFade.value = String(Math.round(dist));
  setFont.value = settings.font || "";
  setMono.value = settings.mono || "";
  settingsSwatches.querySelectorAll(".settings-swatch").forEach((b) =>
    b.classList.toggle("on", b.dataset.c === setTint.value));
  const curMode = settings.cursor === "line" ? "line" : "block";
  setCursor.querySelectorAll("button").forEach((b) => b.classList.toggle("on", b.dataset.v === curMode));
  const smearOn = settings.smear !== false;
  setSmear.classList.toggle("on", smearOn);
  setSmear.textContent = smearOn ? "On" : "Off";
}

function openSettings() {
  settingsOpen = true;
  settingsPanel.hidden = false;
  syncSettingsControls();
  requestAnimationFrame(() => settingsPanel.classList.add("open"));
}
function closeSettings() {
  if (!settingsOpen) return;
  settingsOpen = false;
  settingsPanel.classList.remove("open");
  setTimeout(() => { settingsPanel.hidden = true; }, 160);
  view.focus();
}

settingsSwatches.innerHTML = SWATCHES.map((c) =>
  `<button type="button" class="settings-swatch" data-c="${c}" style="background:${c}" title="${c}"></button>`).join("");
settingsSwatches.querySelectorAll(".settings-swatch").forEach((b) =>
  b.addEventListener("click", () => {
    settings.tint = b.dataset.c;
    applySettings(); saveSettings(); syncSettingsControls();
  }));
const fontOption = (f) => `<option value="${f}">${f || "iA Writer (default)"}</option>`;
setFont.innerHTML = BODY_FONTS.map(fontOption).join("");
setMono.innerHTML = MONO_FONTS.map(fontOption).join("");

setTint.addEventListener("input", () => { settings.tint = setTint.value; applySettings(); saveSettings(); syncSettingsControls(); });
setDim.addEventListener("input", () => { settings.dim = ((100 - +setDim.value) / 100).toFixed(2); applySettings(); saveSettings(); });
setFade.addEventListener("input", () => { settings.fadeDist = `${+setFade.value}vh`; applySettings(); saveSettings(); });
setFont.addEventListener("change", () => { settings.font = setFont.value; applySettings(); saveSettings(); });
setMono.addEventListener("change", () => { settings.mono = setMono.value; applySettings(); saveSettings(); });
setCursor.querySelectorAll("button").forEach((b) =>
  b.addEventListener("click", () => { settings.cursor = b.dataset.v; applySettings(); saveSettings(); syncSettingsControls(); }));
setSmear.addEventListener("click", () => { settings.smear = settings.smear === false; applySettings(); saveSettings(); syncSettingsControls(); });
document.getElementById("settingsReset").addEventListener("click", () => {
  settings = { ...SETTINGS_DEFAULTS };
  applySettings(); saveSettings(); syncSettingsControls();
});

applySettings();   // restore persisted look on launch

function escapeHtml(text) {
  const map = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#039;" };
  return String(text).replace(/[&<>"']/g, (c) => map[c]);
}

// escape HTML, then wrap case-insensitive literal matches of q in <mark>
function highlight(text, q) {
  const esc = escapeHtml(text);
  if (!q) return esc;
  const re = new RegExp(escapeHtml(q).replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "ig");
  return esc.replace(re, (m) => `<mark class="pi-hit">${m}</mark>`);
}

/* on launch: reopen the last file you were on, else the most-recently-touched */
(async () => {
  try {
    let last = null;
    try { last = localStorage.getItem("jd:last"); } catch {}
    if (last && await openFile(last)) return;
    const res = await fetch("/api/search?q=");
    const data = await res.json();
    if (data.results && data.results.length) openFile(data.results[0].path);
  } catch { /* offline / empty — blank canvas */ }
})();
