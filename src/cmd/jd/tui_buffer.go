package main

import "strings"

type pos struct {
	row int
	col int // rune index within the line
}

func posLess(a, b pos) bool {
	if a.row != b.row {
		return a.row < b.row
	}
	return a.col < b.col
}

type snapshot struct {
	lines [][]rune
	cur   pos
}

// editor is a line-oriented rune buffer with a wrap-aware cursor, an anchored
// selection, and undo/redo. Files load and save verbatim (no reflow), so the
// on-disk bytes round-trip unless the user actually edits.
type editor struct {
	lines     [][]rune
	cur       pos
	anchor    pos
	selecting bool
	goalCol   int // preferred column for vertical motion

	path      string
	loadedRaw string
	dirty     bool

	width int // current wrap width in columns

	undo     []snapshot
	redo     []snapshot
	lastKind string
	register string // internal copy/cut register
}

func newEditor() *editor {
	return &editor{lines: [][]rune{{}}, width: 80}
}

func (e *editor) setContent(raw, path string) {
	e.path = path
	e.loadedRaw = raw
	e.lines = splitLines(raw)
	if len(e.lines) == 0 {
		e.lines = [][]rune{{}}
	}
	e.cur = pos{}
	e.anchor = pos{}
	e.selecting = false
	e.dirty = false
	e.undo = nil
	e.redo = nil
	e.lastKind = ""
	e.goalCol = 0
	e.landOnFirstHeading()
}

func splitLines(raw string) [][]rune {
	raw = strings.ReplaceAll(raw, "\r\n", "\n")
	raw = strings.ReplaceAll(raw, "\r", "\n")
	parts := strings.Split(raw, "\n")
	out := make([][]rune, len(parts))
	for i, p := range parts {
		out[i] = []rune(p)
	}
	return out
}

func (e *editor) text() string {
	var b strings.Builder
	for i, ln := range e.lines {
		if i > 0 {
			b.WriteByte('\n')
		}
		b.WriteString(string(ln))
	}
	return b.String()
}

// contentToSave preserves the original bytes when nothing changed.
func (e *editor) contentToSave() string {
	cur := e.text()
	if cur == splitJoin(e.loadedRaw) {
		return e.loadedRaw
	}
	return cur
}

func splitJoin(raw string) string {
	var b strings.Builder
	for i, ln := range splitLines(raw) {
		if i > 0 {
			b.WriteByte('\n')
		}
		b.WriteString(string(ln))
	}
	return b.String()
}

func (e *editor) landOnFirstHeading() {
	inFM := false
	for i, ln := range e.lines {
		s := string(ln)
		if i == 0 && s == "---" {
			inFM = true
			continue
		}
		if inFM {
			if s == "---" {
				inFM = false
			}
			continue
		}
		if strings.HasPrefix(s, "#") {
			e.cur = pos{row: i, col: 0}
			return
		}
	}
}

// ---- selection ----------------------------------------------------------

func (e *editor) startOrKeepSel(extend bool) {
	if extend {
		if !e.selecting {
			e.anchor = e.cur
			e.selecting = true
		}
	} else {
		e.selecting = false
	}
}

func (e *editor) selRange() (pos, pos, bool) {
	if !e.selecting || e.anchor == e.cur {
		return pos{}, pos{}, false
	}
	a, b := e.anchor, e.cur
	if posLess(b, a) {
		a, b = b, a
	}
	return a, b, true
}

func (e *editor) selectedText() string {
	a, b, ok := e.selRange()
	if !ok {
		return ""
	}
	if a.row == b.row {
		return string(e.lines[a.row][a.col:b.col])
	}
	var sb strings.Builder
	sb.WriteString(string(e.lines[a.row][a.col:]))
	for r := a.row + 1; r < b.row; r++ {
		sb.WriteByte('\n')
		sb.WriteString(string(e.lines[r]))
	}
	sb.WriteByte('\n')
	sb.WriteString(string(e.lines[b.row][:b.col]))
	return sb.String()
}

func (e *editor) deleteSelection() bool {
	a, b, ok := e.selRange()
	if !ok {
		return false
	}
	tail := append([]rune(nil), e.lines[b.row][b.col:]...)
	head := append([]rune(nil), e.lines[a.row][:a.col]...)
	merged := append(head, tail...)
	e.lines = append(e.lines[:a.row], append([][]rune{merged}, e.lines[b.row+1:]...)...)
	e.cur = a
	e.selecting = false
	return true
}

// ---- undo ---------------------------------------------------------------

func (e *editor) snap() snapshot {
	cp := make([][]rune, len(e.lines))
	for i, ln := range e.lines {
		cp[i] = append([]rune(nil), ln...)
	}
	return snapshot{lines: cp, cur: e.cur}
}

// pushUndo records pre-edit state, coalescing consecutive same-kind edits so a
// run of typing collapses into one undo step.
func (e *editor) pushUndo(kind string) {
	if kind != "" && kind == e.lastKind {
		e.redo = nil
		return
	}
	e.undo = append(e.undo, e.snap())
	if len(e.undo) > 400 {
		e.undo = e.undo[len(e.undo)-400:]
	}
	e.redo = nil
	e.lastKind = kind
}

func (e *editor) doUndo() {
	if len(e.undo) == 0 {
		return
	}
	e.redo = append(e.redo, e.snap())
	s := e.undo[len(e.undo)-1]
	e.undo = e.undo[:len(e.undo)-1]
	e.lines = s.lines
	e.cur = s.cur
	e.selecting = false
	e.lastKind = ""
	e.markDirty()
}

func (e *editor) doRedo() {
	if len(e.redo) == 0 {
		return
	}
	e.undo = append(e.undo, e.snap())
	s := e.redo[len(e.redo)-1]
	e.redo = e.redo[:len(e.redo)-1]
	e.lines = s.lines
	e.cur = s.cur
	e.selecting = false
	e.lastKind = ""
	e.markDirty()
}

func (e *editor) markDirty() { e.dirty = e.text() != splitJoin(e.loadedRaw) }

// clampCursor keeps cur/anchor inside the buffer after edits that rewrite lines
// without repositioning the caret (e.g. replace-all, table reformat).
func (e *editor) clampCursor() {
	clamp := func(p pos) pos {
		if p.row < 0 {
			p.row = 0
		}
		if p.row >= len(e.lines) {
			p.row = len(e.lines) - 1
		}
		if p.col < 0 {
			p.col = 0
		}
		if p.col > len(e.lines[p.row]) {
			p.col = len(e.lines[p.row])
		}
		return p
	}
	e.cur = clamp(e.cur)
	e.anchor = clamp(e.anchor)
}

// ---- edits --------------------------------------------------------------

func (e *editor) insertRune(r rune) {
	e.pushUndo("insert")
	e.deleteSelection()
	line := e.lines[e.cur.row]
	nl := make([]rune, 0, len(line)+1)
	nl = append(nl, line[:e.cur.col]...)
	nl = append(nl, r)
	nl = append(nl, line[e.cur.col:]...)
	e.lines[e.cur.row] = nl
	e.cur.col++
	e.goalCol = e.cur.col
	e.markDirty()
}

func (e *editor) insertText(s string) {
	if s == "" {
		return
	}
	e.pushUndo("paste")
	e.lastKind = "" // paste is one atomic step
	e.deleteSelection()
	for _, r := range s {
		if r == '\n' {
			e.splitLineAt()
			continue
		}
		if r == '\r' {
			continue
		}
		line := e.lines[e.cur.row]
		nl := make([]rune, 0, len(line)+1)
		nl = append(nl, line[:e.cur.col]...)
		nl = append(nl, r)
		nl = append(nl, line[e.cur.col:]...)
		e.lines[e.cur.row] = nl
		e.cur.col++
	}
	e.goalCol = e.cur.col
	e.markDirty()
}

func (e *editor) splitLineAt() {
	line := e.lines[e.cur.row]
	head := append([]rune(nil), line[:e.cur.col]...)
	tail := append([]rune(nil), line[e.cur.col:]...)
	e.lines[e.cur.row] = head
	rest := append([][]rune{tail}, e.lines[e.cur.row+1:]...)
	e.lines = append(e.lines[:e.cur.row+1], rest...)
	e.cur.row++
	e.cur.col = 0
}

func (e *editor) insertNewline() {
	e.pushUndo("newline")
	e.lastKind = ""
	e.deleteSelection()
	e.splitLineAt()
	e.goalCol = 0
	e.markDirty()
}

func (e *editor) hasSelection() bool {
	return e.selecting && e.anchor != e.cur
}

func (e *editor) backspace() {
	if e.hasSelection() {
		e.pushUndo("")
		e.deleteSelection()
		e.markDirty()
		return
	}
	e.pushUndo("delete")
	if e.cur.col > 0 {
		line := e.lines[e.cur.row]
		e.lines[e.cur.row] = append(line[:e.cur.col-1], line[e.cur.col:]...)
		e.cur.col--
	} else if e.cur.row > 0 {
		prev := e.lines[e.cur.row-1]
		e.cur.col = len(prev)
		e.lines[e.cur.row-1] = append(prev, e.lines[e.cur.row]...)
		e.lines = append(e.lines[:e.cur.row], e.lines[e.cur.row+1:]...)
		e.cur.row--
	}
	e.goalCol = e.cur.col
	e.markDirty()
}

func (e *editor) deleteForward() {
	if e.hasSelection() {
		e.pushUndo("")
		e.deleteSelection()
		e.markDirty()
		return
	}
	e.pushUndo("delete")
	line := e.lines[e.cur.row]
	if e.cur.col < len(line) {
		e.lines[e.cur.row] = append(line[:e.cur.col], line[e.cur.col+1:]...)
	} else if e.cur.row < len(e.lines)-1 {
		e.lines[e.cur.row] = append(line, e.lines[e.cur.row+1]...)
		e.lines = append(e.lines[:e.cur.row+1], e.lines[e.cur.row+2:]...)
	}
	e.markDirty()
}

// ---- movement -----------------------------------------------------------

func (e *editor) curLine() []rune { return e.lines[e.cur.row] }

func (e *editor) moveLeft(extend bool) {
	e.startOrKeepSel(extend)
	if e.cur.col > 0 {
		e.cur.col--
	} else if e.cur.row > 0 {
		e.cur.row--
		e.cur.col = len(e.curLine())
	}
	e.goalCol = e.cur.col
	e.lastKind = ""
}

func (e *editor) moveRight(extend bool) {
	e.startOrKeepSel(extend)
	if e.cur.col < len(e.curLine()) {
		e.cur.col++
	} else if e.cur.row < len(e.lines)-1 {
		e.cur.row++
		e.cur.col = 0
	}
	e.goalCol = e.cur.col
	e.lastKind = ""
}

func isWordRune(r rune) bool {
	return r == '_' || r == '-' ||
		(r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9')
}

func (e *editor) moveWordLeft(extend bool) {
	e.startOrKeepSel(extend)
	if e.cur.col == 0 {
		if e.cur.row > 0 {
			e.cur.row--
			e.cur.col = len(e.curLine())
		}
	} else {
		line := e.curLine()
		i := e.cur.col
		for i > 0 && !isWordRune(line[i-1]) {
			i--
		}
		for i > 0 && isWordRune(line[i-1]) {
			i--
		}
		e.cur.col = i
	}
	e.goalCol = e.cur.col
	e.lastKind = ""
}

func (e *editor) moveWordRight(extend bool) {
	e.startOrKeepSel(extend)
	line := e.curLine()
	if e.cur.col >= len(line) {
		if e.cur.row < len(e.lines)-1 {
			e.cur.row++
			e.cur.col = 0
		}
	} else {
		i := e.cur.col
		for i < len(line) && !isWordRune(line[i]) {
			i++
		}
		for i < len(line) && isWordRune(line[i]) {
			i++
		}
		e.cur.col = i
	}
	e.goalCol = e.cur.col
	e.lastKind = ""
}

// wrapStarts returns the rune offset at which each visual row of a logical line
// begins. Word-wrapping with a hard fallback for over-long tokens.
func wrapStarts(line []rune, width int) []int {
	if width < 1 {
		width = 1
	}
	if len(line) == 0 {
		return []int{0}
	}
	starts := []int{0}
	i := 0
	for i < len(line) {
		end := i + width
		if end >= len(line) {
			break
		}
		brk := -1
		for j := end; j > i; j-- {
			if line[j-1] == ' ' {
				brk = j
				break
			}
		}
		if brk <= i {
			brk = end
		}
		starts = append(starts, brk)
		i = brk
	}
	return starts
}

// visualRow finds which wrap segment col lives in and its column within it.
func visualRow(line []rune, width, col int) (seg, vcol int, starts []int) {
	starts = wrapStarts(line, width)
	seg = 0
	for i := 0; i < len(starts); i++ {
		if col >= starts[i] {
			seg = i
		} else {
			break
		}
	}
	return seg, col - starts[seg], starts
}

func segEnd(line []rune, starts []int, seg int) int {
	if seg+1 < len(starts) {
		return starts[seg+1]
	}
	return len(line)
}

func (e *editor) moveUp(extend bool) {
	e.startOrKeepSel(extend)
	line := e.curLine()
	seg, _, starts := visualRow(line, e.width, e.cur.col)
	if seg > 0 {
		e.cur.col = clampToSeg(line, starts, seg-1, e.goalCol)
	} else if e.cur.row > 0 {
		e.cur.row--
		pl := e.curLine()
		ps := wrapStarts(pl, e.width)
		e.cur.col = clampToSeg(pl, ps, len(ps)-1, e.goalCol)
	}
	e.lastKind = ""
}

func (e *editor) moveDown(extend bool) {
	e.startOrKeepSel(extend)
	line := e.curLine()
	seg, _, starts := visualRow(line, e.width, e.cur.col)
	if seg < len(starts)-1 {
		e.cur.col = clampToSeg(line, starts, seg+1, e.goalCol)
	} else if e.cur.row < len(e.lines)-1 {
		e.cur.row++
		nl := e.curLine()
		ns := wrapStarts(nl, e.width)
		e.cur.col = clampToSeg(nl, ns, 0, e.goalCol)
	}
	e.lastKind = ""
}

// clampToSeg maps a goal column onto a wrap segment, clamped to its extent
// (excluding a trailing space that belongs to the wrap break).
func clampToSeg(line []rune, starts []int, seg, goal int) int {
	start := starts[seg]
	end := segEnd(line, starts, seg)
	maxCol := end
	// don't land past a soft-wrap space seam
	if seg+1 < len(starts) && end > start && line[end-1] == ' ' {
		maxCol = end - 1
	}
	col := start + goal
	if col > maxCol {
		col = maxCol
	}
	if col < start {
		col = start
	}
	return col
}

func (e *editor) moveHome(extend bool) {
	e.startOrKeepSel(extend)
	line := e.curLine()
	seg, _, starts := visualRow(line, e.width, e.cur.col)
	e.cur.col = starts[seg]
	e.goalCol = 0
	e.lastKind = ""
}

// moveEnd goes to the visual row end first, then to the logical line end when
// pressed again — "smart end".
func (e *editor) moveEnd(extend bool) {
	e.startOrKeepSel(extend)
	line := e.curLine()
	seg, _, starts := visualRow(line, e.width, e.cur.col)
	end := segEnd(line, starts, seg)
	if seg+1 < len(starts) && end > starts[seg] && line[end-1] == ' ' {
		end--
	}
	if e.cur.col == end && end < len(line) {
		end = len(line)
	}
	e.cur.col = end
	e.goalCol = e.cur.col
	e.lastKind = ""
}

func (e *editor) movePage(down bool, rows int, extend bool) {
	e.startOrKeepSel(extend)
	for i := 0; i < rows; i++ {
		if down {
			e.moveDown(extend)
		} else {
			e.moveUp(extend)
		}
	}
	e.lastKind = ""
}

func (e *editor) moveDocStart(extend bool) {
	e.startOrKeepSel(extend)
	e.cur = pos{}
	e.goalCol = 0
	e.lastKind = ""
}

func (e *editor) moveDocEnd(extend bool) {
	e.startOrKeepSel(extend)
	e.cur = pos{row: len(e.lines) - 1, col: len(e.lines[len(e.lines)-1])}
	e.goalCol = e.cur.col
	e.lastKind = ""
}
