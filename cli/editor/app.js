import { EditorView, Decoration, ViewPlugin, WidgetType, keymap, drawSelection,
         placeholder } from "@codemirror/view";
import { EditorState, EditorSelection, StateField, StateEffect } from "@codemirror/state";
import { syntaxTree } from "@codemirror/language";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { SearchCursor } from "@codemirror/search";
import { history, defaultKeymap, historyKeymap,
         cursorGroupLeft, cursorGroupRight,
         selectGroupLeft, selectGroupRight,
         cursorCharRight,
         cursorLineBoundaryBackward, cursorLineBoundaryForward,
         selectLineBoundaryBackward, selectLineBoundaryForward } from "@codemirror/commands";
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

// Range from the caret to the end of the deepest block it sits in — so the
// text *after* the cursor in the active block fades like the rest, leaving
// only what you've written so far fully lit. Only when there's a bare caret
// (no selection); returns null otherwise.
function caretBlockTail(state) {
  const r = state.selection.main;
  if (!r.empty) return null;
  const tree = syntaxTree(state);
  let block = null;
  for (const side of [-1, 1]) {
    for (let n = tree.resolveInner(r.head, side); n; n = n.parent) {
      if (BLOCK_NODES.has(n.name)) {
        if (!block || (n.to - n.from) < (block.to - block.from)) block = n;
        break;
      }
    }
  }
  if (!block) return null;
  const end = Math.min(block.to, state.doc.length);
  return end > r.head ? { from: r.head, to: end } : null;
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
    if (blockActive(b, activeSet)) continue;
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

/* Raw-markdown table editing. The caret edits the source directly (live preview
 * reveals it when you enter the block); these keys make rows/columns quick and
 * keep the source aligned. Tab → next cell (wraps to the next row, never adds a
 * column); Shift-Tab → insert a column before the caret; Enter → row after (empty
 * row exits); Shift-Enter → insert a row before; Backspace/Delete in an empty cell
 * whose whole column or row is empty → drop that column/row. Re-serialized aligned. */

// The table block at the caret (collapsed only) + the caret's grid row/column.
// grid === [header, ...dataRows]; gr 0 is the header line, gr≥1 the data rows.
function tableCtx(state) {
  const sel = state.selection.main;
  if (!sel.empty) return null;
  const line = state.doc.lineAt(sel.head);
  const block = findTableBlocks(state).find((b) => line.number >= b.from && line.number <= b.to);
  if (!block) return null;
  const { rows, aligns } = parseTableBlock(state, block.from, block.to);
  const onSeparator = line.number === block.from + 1;
  const gr = line.number <= block.from + 1 ? 0 : line.number - (block.from + 1);
  let pipes = 0;
  const off = sel.head - line.from;
  for (let i = 0; i < off && i < line.text.length; i++) if (line.text[i] === "|" && line.text[i - 1] !== "\\") pipes++;
  const lead = line.text.trimStart().startsWith("|") ? 1 : 0;
  const col = Math.max(0, Math.min(pipes - lead, aligns.length - 1));
  return {
    rows, aligns, gr, col, onSeparator,
    fromPos: state.doc.line(block.from).from,
    toPos: state.doc.line(block.to).to,
  };
}

// Content-start offset of column `c` within an already-serialized `| … | … |` row.
function cellStart(lineText, c) {
  const pipes = [];
  for (let i = 0; i < lineText.length; i++) if (lineText[i] === "|" && lineText[i - 1] !== "\\") pipes.push(i);
  if (pipes.length < 2) return lineText.length;
  const ci = Math.min(c, pipes.length - 2);
  return Math.min(pipes[ci] + 2, pipes[ci + 1]);
}

// Re-serialize the mutated grid, replace the block, drop the caret in (gr, col).
function commitTable(view, ctx, grid, aligns, gr, col) {
  const text = serializeTable(grid, aligns);
  const lines = text.split("\n");
  const li = Math.min(gr === 0 ? 0 : gr + 1, lines.length - 1);   // header, sep, then data
  let off = 0;
  for (let i = 0; i < li; i++) off += lines[i].length + 1;
  const anchor = ctx.fromPos + off + cellStart(lines[li], col);
  view.dispatch({ changes: { from: ctx.fromPos, to: ctx.toPos, insert: text }, selection: { anchor }, scrollIntoView: true });
  return true;
}

function tableTab(view) {
  const ctx = tableCtx(view.state);
  if (!ctx) return false;
  const grid = ctx.rows.map((r) => r.slice()), aligns = ctx.aligns.slice();
  let gr = ctx.gr, col = ctx.col;
  if (col < aligns.length - 1) col++;                  // next cell in the row
  else if (gr < grid.length - 1) { gr++; col = 0; }    // wrap to the next row — never a new column
  // else: last cell — stay put. Columns are created only with Shift-Tab.
  return commitTable(view, ctx, grid, aligns, gr, col);
}
function tableShiftTab(view) {
  const ctx = tableCtx(view.state);
  if (!ctx) return false;
  const grid = ctx.rows.map((r) => r.slice()), aligns = ctx.aligns.slice();
  const at = ctx.col + 1;                           // insert a column AFTER the caret (behind, not in front)
  grid.forEach((r) => r.splice(at, 0, ""));
  aligns.splice(at, 0, "");
  return commitTable(view, ctx, grid, aligns, ctx.gr, at);
}
function tableEnter(view) {
  const ctx = tableCtx(view.state);
  if (!ctx) return false;
  const grid = ctx.rows.map((r) => r.slice()), aligns = ctx.aligns.slice();
  if (ctx.gr >= 1 && grid[ctx.gr].every((c) => c.trim() === "")) {   // empty row → leave the table
    grid.splice(ctx.gr, 1);
    const text = serializeTable(grid, aligns);
    const atEOF = ctx.toPos >= view.state.doc.length;
    view.dispatch({
      changes: { from: ctx.fromPos, to: ctx.toPos, insert: atEOF ? text + "\n" : text },
      selection: { anchor: ctx.fromPos + text.length + 1 },
      scrollIntoView: true,
    });
    return true;
  }
  const at = ctx.gr + 1;
  grid.splice(at, 0, new Array(aligns.length).fill(""));
  return commitTable(view, ctx, grid, aligns, at, 0);
}
function tableShiftEnter(view) {
  const ctx = tableCtx(view.state);
  if (!ctx) return false;
  const grid = ctx.rows.map((r) => r.slice()), aligns = ctx.aligns.slice();
  const at = Math.max(1, ctx.gr + 1);   // insert a row AFTER the caret (behind, not in front; never above the header)
  grid.splice(at, 0, new Array(aligns.length).fill(""));
  return commitTable(view, ctx, grid, aligns, at, 0);
}
// Backspace/Delete inside the table: if the caret sits in an empty cell whose
// whole column (or whole data row) is also empty, drop that column/row in one
// keypress instead of chewing through pipes — so an accidental empty column (e.g.
// a stray Shift-Tab) or empty row cleans up cleanly. Otherwise fall through to the
// normal character delete (returns false so the default keymap handles it).
function tableDelete(view) {
  const ctx = tableCtx(view.state);
  if (!ctx || ctx.onSeparator) return false;
  const grid = ctx.rows.map((r) => r.slice()), aligns = ctx.aligns.slice();
  const { gr, col } = ctx;
  if ((grid[gr]?.[col] ?? "").trim() !== "") return false;   // cell has content → normal delete
  const colEmpty = grid.every((r) => (r[col] ?? "").trim() === "");
  if (colEmpty && aligns.length > 1) {
    grid.forEach((r) => r.splice(col, 1));
    aligns.splice(col, 1);
    return commitTable(view, ctx, grid, aligns, gr, Math.min(col, aligns.length - 1));
  }
  const rowEmpty = (grid[gr] ?? []).every((c) => c.trim() === "");
  if (rowEmpty && gr >= 1 && grid.length > 1) {
    grid.splice(gr, 1);
    return commitTable(view, ctx, grid, aligns, Math.min(gr, grid.length - 1), col);
  }
  return false;
}
const tableEditKeymap = [
  { key: "Tab", run: tableTab, shift: tableShiftTab },
  { key: "Enter", run: tableEnter, shift: tableShiftEnter },
  { key: "Backspace", run: (v) => tableDelete(v) },
  { key: "Delete", run: (v) => tableDelete(v) },
];
// Vertical arrows ENTER a rendered table (revealing it as raw markdown to edit)
// rather than skating past the rendered block; horizontal arrows still glide over
// it at a line edge. Both only fire from OUTSIDE a table — once the caret is on a
// raw table line these bail, so motion within the source is untouched.
const lineInTable = (state, n) => findTableBlocks(state).find((b) => n >= b.from && n <= b.to);
const pastTable = (state, b, forward) => forward
  ? (b.to < state.doc.lines ? state.doc.line(b.to + 1).from : state.doc.line(b.to).to)
  : (b.from > 1 ? state.doc.line(b.from - 1).to : 0);
function tableEnterVertical(view, forward) {
  const s = view.state.selection.main;
  if (!s.empty) return false;
  const curN = view.state.doc.lineAt(s.head).number;
  if (lineInTable(view.state, curN)) return false;   // already editing raw inside → default motion
  const target = view.moveVertically(s, forward);
  const tgtN = view.state.doc.lineAt(target.head).number;
  // Catch a table this step reaches OR jumps over (the rendered block has no text
  // rows of its own, so moveVertically can land past it) — then enter its near edge.
  const blk = findTableBlocks(view.state).find((b) => forward
    ? b.from > curN && b.from <= Math.max(tgtN, curN + 1)
    : b.to < curN && b.to >= Math.min(tgtN, curN - 1));
  if (!blk) return false;
  const entry = forward ? blk.from : blk.to;
  view.dispatch({ selection: { anchor: view.state.doc.line(entry).from }, scrollIntoView: true });
  return true;
}
function tableSkipHorizontal(view, forward) {
  const s = view.state.selection.main;
  if (!s.empty) return false;
  const line = view.state.doc.lineAt(s.head);
  if (lineInTable(view.state, line.number)) return false;
  if (forward ? s.head !== line.to : s.head !== line.from) return false;            // only at a line edge
  const blk = lineInTable(view.state, line.number + (forward ? 1 : -1));
  if (!blk) return false;
  view.dispatch({ selection: { anchor: pastTable(view.state, blk, forward) }, scrollIntoView: true });
  return true;
}
const tableMotionKeymap = [
  { key: "ArrowDown", run: (v) => tableEnterVertical(v, true) },
  { key: "ArrowUp", run: (v) => tableEnterVertical(v, false) },
  { key: "ArrowRight", run: (v) => tableSkipHorizontal(v, true) },
  { key: "ArrowLeft", run: (v) => tableSkipHorizontal(v, false) },
];

class TableWidget extends WidgetType {
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
  ignoreEvent() { return false; }   // let the mousedown below place the caret
  toDOM(view) {
    const headerLine = view.state.doc.lineAt(this.from).number;
    const table = document.createElement("table");
    table.className = "cm-md-table";
    const mkRow = (cells, tag, lineNo) => {
      const tr = document.createElement("tr");
      tr.dataset.line = String(lineNo);
      cells.forEach((c, ci) => {
        const el = document.createElement(tag);
        if (this.aligns[ci]) el.style.textAlign = this.aligns[ci];
        el.innerHTML = renderCell(c) || "";
        tr.appendChild(el);
      });
      return tr;
    };
    const thead = document.createElement("thead");
    thead.appendChild(mkRow(this.rows[0] || [], "th", headerLine));
    table.appendChild(thead);
    const tbody = document.createElement("tbody");
    for (let r = 1; r < this.rows.length; r++) tbody.appendChild(mkRow(this.rows[r], "td", headerLine + 1 + r));
    table.appendChild(tbody);
    // Click a rendered cell -> drop the CM caret on that source line; the block
    // goes active and reveals the raw markdown to edit.
    table.addEventListener("mousedown", (e) => {
      e.preventDefault();
      const tr = e.target.closest("tr");
      const lineNo = tr ? Math.min(+tr.dataset.line, view.state.doc.lines) : null;
      view.dispatch({ selection: { anchor: lineNo ? view.state.doc.line(lineNo).from : this.from } });
      view.focus();
    });
    return table;
  }
}

const tableField = StateField.define({
  create(state) { return buildTableDecos(state); },
  update(value, tr) {
    return (tr.docChanged || tr.selection) ? buildTableDecos(tr.state) : value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

function buildTableDecos(state) {
  const activeSet = activeLineSet(state);
  const decos = [];
  for (const b of findTableBlocks(state)) {
    if (blockActive(b, activeSet)) continue;   // caret inside -> leave raw source to edit
    const fromPos = state.doc.line(b.from).from;
    const toPos = state.doc.line(b.to).to;
    const { rows, aligns } = parseTableBlock(state, b.from, b.to);
    decos.push(Decoration.replace({ block: true, widget: new TableWidget(rows, aligns, fromPos, toPos) }).range(fromPos, toPos));
  }
  return Decoration.set(decos, true);
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
  const tail = caretBlockTail(state);          // dim the active block past the caret
  const decos = [];

  if (tail) {
    // Gradient fade across the active block's tail: full text colour right at
    // the caret, easing to the focus-dim colour at the block's last glyph. One
    // mark per character carries its position fraction (--t); CSS interpolates
    // the opacity so --dim stays the single source of truth.
    const text = state.doc.sliceString(tail.from, tail.to);
    const span = Math.max(text.length - 1, 1);
    for (let i = 0; i < text.length; i++) {
      if (text[i] === "\n") continue;
      const t = i / span;
      decos.push(Decoration.mark({
        class: "cm-dim-after",
        attributes: { style: `--t:${t.toFixed(4)}` },
      }).range(tail.from + i, tail.from + i + 1));
    }
  }

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
let redrawSelection = () => {};   // custom rounded selection blob; real impl below
let centerRaf = null;
function scheduleCenter() {
  if (centerRaf !== null) return;
  centerRaf = requestAnimationFrame(() => { centerRaf = null; centerCaret(true); });
}
// Hoisted here because the view's initial update fires updateRead()
// during construction — a `let` declared after the editor would still be in its
// temporal dead zone and throw, blanking the whole UI.
let readRaf = null;
// Hoisted: the update listener reads findOpen during the view's initial
// construction update, so it must be initialized before the view is built.
let findOpen = false;
// Cursor look + smear, driven by the settings overlay (applySettings sets them).
// Hoisted for the same reason as findOpen — the update listener reads them
// during the view's initial construction update.
let cursorBlock = false;
let smearEnabled = true;
let cursorBlockW = 0;       // measured glyph width under the caret (block mode)
let smearSkipNext = false;  // suppress one smear (e.g. the programmatic load-time jump)
// Smear feel, derived from the Trail/Speed settings (set in applySettings).
// smearDuration is how long (ms) the source rect takes to catch the destination —
// longer = a longer-lived trail; smearShrink is the diagonal taper sharpness.
let smearDuration = 320, smearShrink = 0.77;

// CM rewrites .cm-editor's className wholesale on focus/state changes, wiping
// any class we add there. So the block-cursor flag and its measured width ride
// the editor HOST (which CM never touches); --cursor-w inherits down into the
// caret, and the selector is .editor-host.cursor-block .cm-cursor.
const editorHost = document.getElementById("editor");

// Block cursor legibility: tag the single glyph under an empty caret so CSS can
// repaint it in --cursor-fg (the computed contrast colour). The mark is always
// emitted; only the .cursor-block rule colours it, so switching cursor modes
// needs no rebuild. Skips selections and line ends (no glyph sits under those).
const cursorGlyphMark = Decoration.mark({ class: "cm-cursor-glyph" });
function cursorGlyphDeco(view) {
  const sel = view.state.selection.main;
  if (!sel.empty) return Decoration.none;
  const line = view.state.doc.lineAt(sel.head);
  if (sel.head >= line.to) return Decoration.none;
  return Decoration.set([cursorGlyphMark.range(sel.head, sel.head + 1)]);
}
// The glyph marks the char at the caret offset for block-cursor contrast. At a
// soft-wrap seam that offset renders on the NEXT visual row while the caret draws
// at the END of THIS one (assoc -1, e.g. after End) — recolouring it would blank
// a char with no block behind it. Reading layout during update() is forbidden, so
// the plugin marks optimistically and a measure pass (layout-legal, pre-paint)
// flags the element when its row differs from the caret's; CSS cancels the recolour.
const cursorGlyphPlugin = ViewPlugin.fromClass(class {
  constructor(view) { this.decorations = cursorGlyphDeco(view); this.sync(view); }
  update(u) {
    if (u.selectionSet || u.docChanged || u.viewportChanged) this.decorations = cursorGlyphDeco(u.view);
    if (u.selectionSet || u.docChanged || u.viewportChanged || u.geometryChanged) this.sync(u.view);
  }
  sync(view) {
    view.requestMeasure({
      key: "cursorGlyphSplit",
      read: () => {
        const sel = view.state.selection.main;
        if (!sel.empty) return false;
        const caret = view.coordsAtPos(sel.head, sel.assoc || -1);
        const glyph = view.coordsAtPos(sel.head, 1);
        return !!(caret && glyph && Math.abs(caret.top - glyph.top) > 1);
      },
      write: (split) => {
        const el = view.dom.querySelector(".cm-cursor-glyph");
        if (el) el.classList.toggle("cm-cursor-glyph-off", split);
      },
    });
  }
}, { decorations: (v) => v.decorations });

// Soft-wrap seam: one document offset that renders on two visual rows (end of the
// upper row / start of the lower). True only when the position straddles rows.
function atWrapSeam(view, pos) {
  const a = view.coordsAtPos(pos, -1), b = view.coordsAtPos(pos, 1);
  return !!(a && b && Math.abs(a.top - b.top) > 1);
}
// Rightward motion that never skips a wrapped row's first character. CM shares the
// seam offset between rows and biases rightward travel to the upper row's END, so
// the lower row's first glyph gets jumped. Here we re-associate the caret to the
// lower row's START (same offset, assoc +1) instead — both when already parked at
// the seam (e.g. after End) and when a normal step lands on it.
function cursorCharRightWrap(view) {
  const r0 = view.state.selection.main;
  if (r0.empty && (r0.assoc || 0) < 0 && atWrapSeam(view, r0.head)) {
    view.dispatch({ selection: EditorSelection.cursor(r0.head, 1), scrollIntoView: true });
    return true;
  }
  const handled = cursorCharRight(view);
  const r = view.state.selection.main;
  if (handled && r.empty && (r.assoc || 0) < 0 && atWrapSeam(view, r.head))
    view.dispatch({ selection: EditorSelection.cursor(r.head, 1), scrollIntoView: true });
  return handled;
}

// End: first press -> end of the visual wrapped row; if already there -> end of the
// whole logical line (the reflowed paragraph), so a wrapped prose line still has a
// reachable "true" end. Shift+End extends the same way.
// Visual wrapped-row end of `head`. assoc -1 makes a soft-wrap seam resolve to the
// row the caret is ENDING (so an offset shared by two rows isn't read as the lower
// row's start), which is what lets the "already at row end?" test below converge.
const visualRowEnd = (view, head) => view.moveToLineBoundary(EditorSelection.cursor(head, -1), true).head;
// Target: the visual wrapped-row end; if the caret is already there, the end of the
// whole logical line (paragraph) instead.
function smartEndTarget(view, head) {
  const rowEnd = visualRowEnd(view, head);
  const logicalEnd = view.state.doc.lineAt(head).to;
  return (head === rowEnd && head !== logicalEnd) ? logicalEnd : rowEnd;
}
function endSmart(view) {
  const head = view.state.selection.main.head;
  view.dispatch({ selection: EditorSelection.cursor(smartEndTarget(view, head), -1), scrollIntoView: true });
  return true;
}

const view = new EditorView({
  parent: editorHost,
  state: EditorState.create({
    doc: "",
    extensions: [
      history(),
      drawSelection(),
      EditorView.lineWrapping,
      markdown({ base: markdownLanguage, addKeymap: true }),
      tableField,
      livePreview,
      cursorGlyphPlugin,
      placeholder("Press ⌘K to open a .jd file, or just start writing…"),
      keymap.of([
        { key: "Mod-s", preventDefault: true, run: () => { saveFile(); return true; } },
        // swallow CM's default Ctrl-k (deleteLine) so the global search shortcut
        // wins on Windows/Linux; the window handler below does the focus.
        { key: "Mod-k", preventDefault: true, run: () => true },
        // Alt/Option + ←/→ jump by word (Shift extends the selection)
        { key: "Alt-ArrowLeft", run: cursorGroupLeft, shift: selectGroupLeft, preventDefault: true },
        { key: "Alt-ArrowRight", run: cursorGroupRight, shift: selectGroupRight, preventDefault: true },
        // Home/End jump to the wrapped VISUAL-row boundary under lineWrapping,
        // not the logical line edge (which would skip past the wrap).
        { key: "Home", run: cursorLineBoundaryBackward, shift: selectLineBoundaryBackward, preventDefault: true },
        { key: "End", run: endSmart, shift: selectLineBoundaryForward, preventDefault: true },
        // ArrowRight: don't skip the first glyph of a wrapped row at the seam.
        { key: "ArrowRight", run: cursorCharRightWrap },
        ...tableMotionKeymap,
        ...tableEditKeymap,
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
        if (u.selectionSet || u.docChanged) { scheduleCenter(); updateCursorBlockWidth(); smearMove(); redrawSelection(); updateRead(); }
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
  const scroller = view.scrollDOM;
  let target = scroller.scrollTop;
  let raf = null;
  const max = () => scroller.scrollHeight - scroller.clientHeight;
  const clamp = (v) => Math.max(0, Math.min(max(), v));

  // Left mouse held = the user is drag-selecting. Typewriter follow is suspended
  // for the duration (see centerCaret), but when the cursor nears a viewport edge
  // we run a slow edge-autoscroll so the content under the growing selection stays
  // visible. Keyboard selection still follows the caret normally.
  let dragging = false, dragX = 0, dragY = 0, dragRaf = null;
  const EDGE = 72, EDGE_MAX = 9;            // edge band (px) and top speed (px/frame)
  function edgeScroll() {
    if (!dragging) { dragRaf = null; return; }
    const rect = scroller.getBoundingClientRect();
    let f = 0;                                          // -1..1, signed depth into the edge band
    if (dragY < rect.top + EDGE)         f = -(rect.top + EDGE - dragY) / EDGE;
    else if (dragY > rect.bottom - EDGE) f =  (dragY - (rect.bottom - EDGE)) / EDGE;
    if (f !== 0) {
      const next = clamp(scroller.scrollTop + Math.max(-1, Math.min(1, f)) * EDGE_MAX);
      if (next !== scroller.scrollTop) {
        scroller.scrollTop = next; target = next;       // keep typewriter target synced
        // mouse is stationary at the edge, so CM fires no pointer event — extend
        // the selection ourselves to whatever now sits under the cursor.
        const pos = view.posAtCoords({ x: dragX, y: dragY });
        if (pos != null && pos !== view.state.selection.main.head)
          view.dispatch({ selection: { anchor: view.state.selection.main.anchor, head: pos } });
      }
    }
    dragRaf = requestAnimationFrame(edgeScroll);
  }
  scroller.addEventListener("mousedown", (e) => {
    if (e.button !== 0) return;
    dragging = true; dragX = e.clientX; dragY = e.clientY;
    if (dragRaf === null) dragRaf = requestAnimationFrame(edgeScroll);
  });
  addEventListener("mousemove", (e) => { if (dragging) { dragX = e.clientX; dragY = e.clientY; } });
  addEventListener("mouseup", (e) => { if (e.button === 0) dragging = false; });

  function tick() {
    const cur = scroller.scrollTop;
    const next = cur + (target - cur) * 0.18;         // lerp → momentum glide
    if (Math.abs(target - next) < 0.4) { scroller.scrollTop = target; raf = null; return; }
    scroller.scrollTop = next;
    raf = requestAnimationFrame(tick);
  }
  function glideTo(t) { target = clamp(t); if (raf === null) raf = requestAnimationFrame(tick); }

  // wheel → momentum (also lets you scroll away from the centered line to read)
  {
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
    if (dragging) {
      // Mouse drag-selecting — don't recenter; the edge-autoscroll loop owns
      // scrolling here. Abort any in-flight momentum glide (e.g. the recenter
      // kicked off by the initial click) and resync the target so the typewriter
      // loop stops fighting the drag.
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
    if (animated) glideTo(t);
    else { scroller.scrollTop = t; target = t; }
  };
  centerCaret(false);                                 // center whatever's loaded now
})();

/* ------------------------------------------------------------------ *
 *  Neovide-style cursor smear (the smear-cursor.nvim spring model). The cursor
 *  cell is a quad of four corners; on a move each corner springs toward the new
 *  cell, but stiffness is ranked by closeness to the destination centre, so the
 *  leading corners rush ahead while the trailing ones lag — the block stretches
 *  along its path then contracts back into a cell. The quad IS the cursor while
 *  travelling (CM's caret is hidden); corners live in scroll-invariant content
 *  coords so the smear rides typewriter scrolling, re-projected each frame.
 * ------------------------------------------------------------------ */
// Rounded path around an ordered polygon: each vertex becomes a quadratic arc whose
// radius clamps to half the shorter adjacent edge, so short/thin shapes never self-
// overlap. Shared by the smear quad (4 corners) and the selection blob (staircase).
function roundedQuadPath(pts, r) {
  const n = pts.length;
  if (n < 3) return "";
  let d = "";
  for (let i = 0; i < n; i++) {
    const p = pts[i], prev = pts[(i - 1 + n) % n], next = pts[(i + 1) % n];
    const v1x = prev[0] - p[0], v1y = prev[1] - p[1];
    const v2x = next[0] - p[0], v2y = next[1] - p[1];
    const l1 = Math.hypot(v1x, v1y) || 1, l2 = Math.hypot(v2x, v2y) || 1;
    const rr = Math.min(r, l1 / 2, l2 / 2);
    const ax = p[0] + (v1x / l1) * rr, ay = p[1] + (v1y / l1) * rr;
    const bx = p[0] + (v2x / l2) * rr, by = p[1] + (v2y / l2) * rr;
    d += `${i === 0 ? "M" : "L"} ${ax.toFixed(2)} ${ay.toFixed(2)} Q ${p[0].toFixed(2)} ${p[1].toFixed(2)} ${bx.toFixed(2)} ${by.toFixed(2)} `;
  }
  return d + "Z";
}

(function smearCursor() {
  // Always wire up the engine; whether it actually animates is gated per-move by
  // `smearEnabled` (the Smear setting). Reduced-motion only steers the DEFAULT
  // (see SETTINGS_DEFAULTS) — an explicit opt-in in Settings must still take.
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("class", "cm-smear");
  const path = document.createElementNS(NS, "path");
  svg.appendChild(path);
  document.body.appendChild(svg);

  // Caret cell in scroll-invariant CONTENT coords, so the captured rect keeps
  // tracking the text while the typewriter scroll glides underneath it.
  const caretCell = () => {
    const sel = view.state.selection.main;
    if (!sel.empty) return null;                 // only a collapsed caret has a cell
    // Pass the caret's association so a soft-wrap seam resolves to the same visual
    // row CM draws the caret on (assoc -1 = upper row end), not coordsAtPos's
    // default +1 (lower row start) — otherwise the smear lands a row off.
    const c = view.coordsAtPos(sel.head, sel.assoc < 0 ? -1 : 1);
    if (!c) return null;
    const b = view.scrollDOM.getBoundingClientRect();
    return {
      x: c.left - b.left + view.scrollDOM.scrollLeft,
      y: c.top - b.top + view.scrollDOM.scrollTop,
      w: cursorBlock ? Math.max(cursorBlockW || view.defaultCharacterWidth, 3) : 2,
      h: c.bottom - c.top,
    };
  };
  // content-space point → viewport px
  const toView = (x, y) => {
    const b = view.scrollDOM.getBoundingClientRect();
    return [x + b.left - view.scrollDOM.scrollLeft, y + b.top - view.scrollDOM.scrollTop];
  };

  // The smear shape (smear-cursor.nvim / vsc-smearcursor model): the cursor is a
  // cell rect at BOTH the source and the destination, and the smear is the convex
  // quad BRIDGING them — the two corners of each rect farthest from the OTHER
  // rect's centre, wound by angle. The block streaks from where it was to where it
  // is, then the source rect slides up under the destination and the quad collapses
  // back into one cell. Identical maths for line and block — only the cell width
  // differs (2px vs the measured glyph), so both smear from the same four points.
  const rectAt = (x, y, w, h, ix, iy) => [[x + ix, y + iy], [x + w - ix, y + iy], [x + w - ix, y + h - iy], [x + ix, y + h - iy]];
  const centre = (r) => [(r[0][0] + r[2][0]) / 2, (r[0][1] + r[2][1]) / 2];
  const farthest2 = (c, pts) => pts.slice().sort((a, b) => Math.hypot(b[0] - c[0], b[1] - c[1]) - Math.hypot(a[0] - c[0], a[1] - c[1])).slice(0, 2);
  const windByAngle = (pts) => {
    const cx = pts.reduce((s, p) => s + p[0], 0) / pts.length, cy = pts.reduce((s, p) => s + p[1], 0) / pts.length;
    return pts.slice().sort((a, b) => Math.atan2(a[1] - cy, a[0] - cx) - Math.atan2(b[1] - cy, b[0] - cx));
  };
  const bridge = (tip, tail) => windByAngle([...farthest2(centre(tip), tail), ...farthest2(centre(tail), tip)]);

  // cubic ease-in-out — the source rect's catch-up curve toward the destination.
  const ease = (x) => (x < 0.5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2);
  const SMEAR_RADIUS = 3;   // matches .cursor-block .cm-cursor border-radius

  let prev = null;       // source cell {x,y,w,h} content coords — the smear origin
  let to = null;         // destination cell
  let t = 0, last = 0;   // progress 0..1, last rAF stamp (ms)
  let raf = null;

  // Rounded quad (matches the resting block's border-radius), drawn in absolute
  // viewport coords into the full-viewport SVG overlay. Filled in block mode,
  // stroked in outline mode — both handled by the .cm-smear path CSS.
  const paint = (pts) => { path.setAttribute("d", roundedQuadPath(pts, SMEAR_RADIUS)); };

  const settle = () => {                             // hand back to CM's caret
    editorHost.classList.remove("smearing");
    svg.classList.remove("show");
    if (raf !== null) { cancelAnimationFrame(raf); raf = null; }
  };

  const step = (stamp) => {
    raf = null;
    const dt = last ? stamp - last : 16;
    last = stamp;
    t = Math.min(1, t + dt / Math.max(smearDuration, 1));
    if (t >= 1) { settle(); return; }                // source rect caught up → CM caret resumes
    const e = ease(t);
    const w = to.w, h = to.h;
    // source rect slides from `prev` toward the destination; the tip stays pinned
    // at the destination cell, so the bridging quad shrinks as the two converge.
    const sx = prev.x + (to.x - prev.x) * e, sy = prev.y + (to.y - prev.y) * e;
    // taper only diagonal moves (Neovide's "skew when moving diagonally"): inset
    // both rects, easing the inset back out as the source rect lands.
    const diag = Math.abs(to.x - sx) > 0.5 && Math.abs(to.y - sy) > 0.5;
    const ix = diag ? (1 - e) * (w / 2) * smearShrink : 0;
    const iy = diag ? (1 - e) * (h / 2) * smearShrink : 0;
    const tip = rectAt(to.x, to.y, w, h, ix, iy);
    const tail = rectAt(sx, sy, w, h, ix, iy);
    paint(bridge(tip, tail).map((p) => toView(p[0], p[1])));
    raf = requestAnimationFrame(step);
  };

  // Every move: the caret jumped to a new cell; bridge a quad from where it rested
  // to where it landed and let the source rect catch up. No per-movement-type
  // branches — type, arrow, Home/End, newline, across rows are all one lerp.
  smearMove = () => {
    const cell = caretCell();
    if (!cell || !smearEnabled) {                    // nothing to smear → rest on CM's caret
      prev = null; to = null; settle();
      return;
    }
    if (smearSkipNext) {                             // suppress one move (e.g. the load-time jump)
      smearSkipNext = false; prev = cell; to = null; settle();
      return;
    }
    const origin = to || prev;                       // where the caret rested before this move
    if (!origin) { prev = cell; return; }            // first cell → just remember it
    if (Math.hypot(cell.x - origin.x, cell.y - origin.y) < 0.5) return;   // no real move
    prev = origin;
    to = cell;
    t = 0; last = 0;
    editorHost.classList.add("smearing");             // the bridging quad becomes the cursor
    svg.classList.add("show");
    if (raf === null) raf = requestAnimationFrame(step);
  };
})();

/* ------------------------------------------------------------------ *
 *  Custom selection: CM draws the selection as plain per-line rectangles. We hide
 *  those and redraw the same geometry as ONE rounded outline per contiguous line
 *  stack — a joined "blob" with arced corners. The SVG lives inside the scroller
 *  so it rides scrolling exactly like CM's own (now-hidden) selection rects, and
 *  we read those rects for placement so it always lines up with the text.
 * ------------------------------------------------------------------ */
(function selectionBlob() {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("class", "cm-sel-blob");
  const path = document.createElementNS(NS, "path");
  svg.appendChild(path);
  view.scrollDOM.appendChild(svg);

  const R = 6;   // corner radius; clamped per-corner to half the shorter edge

  const outline = (rs) => {
    rs = rs.slice().sort((a, b) => a.t - b.t);
    const pts = [];
    for (const r of rs) pts.push([r.r, r.t], [r.r, r.b]);
    for (let i = rs.length - 1; i >= 0; i--) pts.push([rs[i].l, rs[i].b], [rs[i].l, rs[i].t]);
    return pts;
  };

  const paint = () => {
    const divs = view.scrollDOM.querySelectorAll(".cm-selectionBackground");
    if (!divs.length) { path.setAttribute("d", ""); return; }
    const sb = view.scrollDOM.getBoundingClientRect();
    const sx = view.scrollDOM.scrollLeft, sy = view.scrollDOM.scrollTop;
    const rects = Array.from(divs).map((el) => {
      const r = el.getBoundingClientRect();
      return { l: r.left - sb.left + sx, t: r.top - sb.top + sy, r: r.right - sb.left + sx, b: r.bottom - sb.top + sy };
    });
    rects.sort((a, b) => a.t - b.t || a.l - b.l);
    const groups = [];
    for (const r of rects) {
      const g = groups[groups.length - 1];
      if (g && r.t <= g[g.length - 1].b + 1) g.push(r); else groups.push([r]);
    }
    path.setAttribute("d", groups.map((g) => roundedQuadPath(outline(g), R)).join(" "));
  };
  redrawSelection = () => {
    view.requestMeasure({ key: "selBlob", read: () => null, write: () => paint() });
  };
  redrawSelection();
})();

/* Block cursor: when enabled, the native caret is widened into a block sized to
   the glyph under it — measured live, since the prose font is proportional. The
   smear above reads the same width, so a moving block streaks like Neovide. */
function measureGlyphWidth(pos) {
  const a = view.coordsAtPos(pos);
  const b = view.coordsAtPos(pos + 1);
  if (a && b && Math.abs(b.top - a.top) < 1 && b.left > a.left) return b.left - a.left;
  // At a line end / wrap, pos+1 sits on the next row — fall back to the glyph
  // BEFORE the caret so the block keeps the line's rhythm instead of popping to
  // the (wider) default character width.
  const p = view.coordsAtPos(pos - 1);
  if (a && p && Math.abs(a.top - p.top) < 1 && a.left > p.left) return a.left - p.left;
  return view.defaultCharacterWidth;
}
function updateCursorBlockWidth() {
  if (!cursorBlock) return;
  cursorBlockW = measureGlyphWidth(view.state.selection.main.head);
  editorHost.style.setProperty("--cursor-w", `${cursorBlockW}px`);
}

/* Writing progress — fill the searchbar's bottom border (--read, 0–1) by the
   caret's character offset through the document (head / total length), so it
   advances with every keystroke and cursor move. Coalesced into a rAF. */
function updateRead() {
  if (readRaf !== null) return;
  readRaf = requestAnimationFrame(() => {
    readRaf = null;
    const total = view.state.doc.length;
    const pos = view.state.selection.main.head;
    searchwrap.style.setProperty("--read", total > 0 ? (pos / total).toFixed(4) : "0");
  });
}
updateRead();

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

// Position of the first ATX heading's `#` (skipping any frontmatter), or null.
function firstHeadingPos(state) {
  const doc = state.doc;
  const fm = frontmatter(state);
  const start = fm ? Math.min(fm.close + 1, doc.lines) : 1;
  for (let n = start; n <= doc.lines; n++) {
    const line = doc.line(n);
    if (/^#{1,6}\s/.test(line.text)) return line.from;
  }
  return null;
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
    // Land the caret on the first heading (the first '#'), else the body start.
    // Hold it hidden, then 50ms after load drop it in and fade it up — by then
    // layout has settled so the block cursor is sized to the right glyph. (Caret /
    // glyph vertical alignment is handled by running the entrance animation on
    // .cm-editor so the cursor layer rides along — see contentIn in style.css.)
    const fm = frontmatter(view.state);
    const fallback = view.state.doc.line(fm ? Math.min(fm.close + 2, view.state.doc.lines) : 1).from;
    const pos = firstHeadingPos(view.state) ?? fallback;
    host.classList.add("cursor-arming");             // caret hidden (no fade) while we wait
    view.focus();
    setTimeout(() => {
      smearSkipNext = true;                          // place without a load-time smear
      view.dispatch({ selection: { anchor: pos } });
      updateCursorBlockWidth();
      centerCaret(false);
      host.classList.remove("cursor-arming");        // fade the correctly-sized caret in
    }, 50);
    // The block width is the glyph advance, which differs from the fallback font —
    // re-measure once the webfont settles (caret position is already stable).
    document.fonts?.ready.then(updateCursorBlockWidth);
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
  setTimeout(() => { if (!searching) resultsEl.hidden = true; }, 280);
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
  setTimeout(() => { confirmEl.hidden = true; }, 280);
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
  setTimeout(() => { findBar.hidden = true; }, 280);
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
  setTimeout(() => { rgPanel.hidden = true; }, 280);
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
const setTrail = document.getElementById("setTrail");
const setSmearSpeed = document.getElementById("setSmearSpeed");
const setTheme = document.getElementById("setTheme");

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
// Smear defaults OFF when the OS asks for reduced motion, ON otherwise. A stored
// choice (loadSettings spreads it over these defaults) always wins, so flipping
// the Smear toggle takes effect regardless of the OS preference.
const SETTINGS_DEFAULTS = { theme: "auto", tint: "", dim: "", fadeDist: "", font: "", mono: "", cursor: "block", smear: !matchMedia("(prefers-reduced-motion: reduce)").matches, smearTrail: 78, smearSpeed: 62 };
const SWATCHES = ["#81a2be", "#de935f", "#b5bd68", "#b294bb"];
let settingsOpen = false;
let settings = loadSettings();

function loadSettings() {
  try {
    const s = { ...SETTINGS_DEFAULTS, ...(JSON.parse(localStorage.getItem(SETTINGS_KEY)) || {}) };
    // Legacy: the standalone `outline` toggle folded into the cursor style.
    if (s.outline === true && s.cursor !== "line") s.cursor = "outline";
    delete s.outline;
    return s;
  } catch { return { ...SETTINGS_DEFAULTS }; }
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
  // A swapped-in webfont changes glyph metrics: re-measure AND re-center the caret
  // + refresh the block width, else the first caret (placed with fallback metrics on
  // load) sits too high until the next cursor move forces a re-center.
  document.fonts?.ready.then(() => { view.requestMeasure(); updateCursorBlockWidth(); centerCaret(false); });
  // cursor look + smear
  cursorBlock = settings.cursor !== "line";
  smearEnabled = settings.smear !== false;
  editorHost.classList.toggle("cursor-block", cursorBlock);
  // "outline" is the hollow block variant — same cell, stroked 2px instead of a
  // solid fill (for both the resting block and its smear).
  document.body.classList.toggle("cursor-outline", settings.cursor === "outline");
  if (cursorBlock) updateCursorBlockWidth(); else editorHost.style.removeProperty("--cursor-w");
  // Block cursor legibility: the glyph sits over a solid --tint fill, so colour it
  // black or white — whichever contrasts the (now-resolved) tint more. Reads the
  // computed value so the default tint's light/dark variant is picked up too.
  root.setProperty("--cursor-fg", contrastOf(getComputedStyle(document.documentElement).getPropertyValue("--tint")));
  // Trail / Speed → smear feel. Speed shortens the catch-up time (overall pace);
  // Trail lengthens it (a longer-lived streak) and sharpens the diagonal taper.
  // Both 0–100 from the sliders.
  const speed01 = (settings.smearSpeed ?? 62) / 100;
  const trail01 = (settings.smearTrail ?? 78) / 100;
  smearDuration = Math.round(70 + trail01 * 260 + (1 - speed01) * 120);   // ~70 (snappy) … ~450ms (long)
  smearShrink = 0.3 + trail01 * 0.6;                                      // 0.3 (subtle) … 0.9 (sharp taper)
}

function hexOf(v) {
  v = (v || "").trim();
  return /^#[0-9a-fA-F]{6}$/.test(v) ? v.toLowerCase() : "#007aff";
}
// WCAG relative luminance → black or white, whichever has the higher contrast
// against `color` (a #rgb / #rrggbb tint). 0.179 is the standard crossover point.
function contrastOf(color) {
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec((color || "").trim());
  if (!m) return "#ffffff";
  const h = m[1].length === 3 ? m[1].replace(/./g, (c) => c + c) : m[1];
  const n = parseInt(h, 16);
  const lin = (c) => { c /= 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
  const L = 0.2126 * lin((n >> 16) & 255) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255);
  return L > 0.179 ? "#000000" : "#ffffff";
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
  setCursor.querySelectorAll("button").forEach((b) => b.classList.toggle("on", b.dataset.v === settings.cursor));
  const smearOn = settings.smear !== false;
  setSmear.classList.toggle("on", smearOn);
  setSmear.textContent = smearOn ? "On" : "Off";
  setTrail.value = String(settings.smearTrail ?? 78);
  setSmearSpeed.value = String(settings.smearSpeed ?? 62);
  settingsPanel.classList.toggle("smear-off", !smearOn);   // hide Trail/Speed when smear is off
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
  setTimeout(() => { settingsPanel.hidden = true; }, 280);
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
setTrail.addEventListener("input", () => { settings.smearTrail = +setTrail.value; applySettings(); saveSettings(); });
setSmearSpeed.addEventListener("input", () => { settings.smearSpeed = +setSmearSpeed.value; applySettings(); saveSettings(); });
document.addEventListener("mousedown", (e) => {
  if (settingsOpen && !settingsPanel.contains(e.target)) closeSettings();
});
document.getElementById("settingsReset").addEventListener("click", () => {
  settings = { ...SETTINGS_DEFAULTS };
  applySettings(); saveSettings(); syncSettingsControls();
});

applySettings();   // restore persisted look on launch
// On auto theme with the default tint, an OS light/dark flip changes the resolved
// --tint, so re-derive --cursor-fg (the only theme-dependent value JS computes).
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", applySettings);

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
