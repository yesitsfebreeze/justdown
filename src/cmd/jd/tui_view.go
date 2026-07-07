package main

import (
	"encoding/base64"
	"fmt"
	"image/color"
	"path/filepath"
	"strings"

	uv "github.com/charmbracelet/ultraviolet"
	justdown "github.com/yesitsfebreeze/justdown/src"
)

func b64(s string) string { return base64.StdEncoding.EncodeToString([]byte(s)) }

// visualLine is one on-screen row: which logical line and its rune span.
type visualLine struct {
	row   int
	start int
	end   int
}

// layoutEditor produces the full list of wrapped visual rows for the buffer.
func (a *app) layoutEditor() []visualLine {
	var out []visualLine
	w := a.editorW()
	for r, line := range a.ed.lines {
		starts := wrapStarts(line, w)
		for i, s := range starts {
			end := len(line)
			if i+1 < len(starts) {
				end = starts[i+1]
			}
			out = append(out, visualLine{row: r, start: s, end: end})
		}
	}
	return out
}

// cursorVisualIndex returns the index into the visual-row list where the cursor
// sits, plus its column offset within that row.
func (a *app) cursorVisual(vls []visualLine) (int, int) {
	for i, vl := range vls {
		if vl.row != a.ed.cur.row {
			continue
		}
		last := i+1 >= len(vls) || vls[i+1].row != a.ed.cur.row
		if a.ed.cur.col >= vl.start && (a.ed.cur.col < vl.end || (last && a.ed.cur.col <= vl.end)) {
			return i, a.ed.cur.col - vl.start
		}
	}
	return 0, 0
}

func (a *app) paint(r *uv.TerminalRenderer) {
	b := a.compose()
	r.Render(b.RenderBuffer)

	if cx, cy, ok := a.cursorScreen(); ok && !a.fnd.active && !a.rg.active && !a.confirmDelete {
		r.MoveTo(cx, cy)
		r.WriteString("\x1b[?25h")
	} else {
		r.WriteString("\x1b[?25l")
	}
	if a.clipboard != "" {
		r.WriteString(a.clipboard)
		a.clipboard = ""
	}
	_ = r.Flush()
}

func (a *app) compose() uv.ScreenBuffer {
	if a.fb.RenderBuffer == nil || a.fb.Width() != a.cols || a.fb.Height() != a.rows {
		a.fb = uv.NewScreenBuffer(a.cols, a.rows)
	}
	b := a.fb
	fill(b, 0, 0, a.cols, a.rows, " ", tst(nil, nil, 0))

	a.drawSearchBar(b)
	a.drawEditor(b)
	a.drawStatus(b)

	if a.pop.active {
		a.drawPopup(b)
	}
	if a.fbar.active {
		a.drawFindBar(b)
	}
	if a.fnd.active {
		a.drawFinder(b)
	}
	if a.rg.active {
		a.drawRg(b)
	}
	if a.confirmDelete {
		a.drawConfirm(b)
	}
	return b
}

func (a *app) drawSearchBar(b uv.ScreenBuffer) {
	barStyle := tst(nil, colBarBG, 0)
	fill(b, 0, 0, a.cols, 1, " ", barStyle)

	if a.fnd.active {
		text(b, 1, 0, "> ", tst(colAccent, colBarBG, 0))
		q := string(a.fnd.query)
		text(b, 3, 0, q, tst(nil, colBarBG, uv.AttrBold))
		return
	}

	title := "no file"
	if a.ed.path != "" {
		title = a.titleFor(a.ed.path)
	}
	hint := "ctrl+k files"
	x := textClip(b, 1, 0, a.cols-len(hint)-5, title, tst(nil, colBarBG, uv.AttrBold))
	if a.ed.dirty {
		text(b, x+1, 0, "*", tst(colDirty, colBarBG, 0))
	}
	text(b, a.cols-len(hint)-1, 0, hint, tst(colMuted, colBarBG, 0))
}

// titleFor renders a path relative to a working root when possible, else the
// home-shortened display path.
func (a *app) titleFor(p string) string {
	for _, r := range a.lib.roots {
		if rel, err := filepath.Rel(r, p); err == nil && !strings.HasPrefix(rel, "..") {
			return filepath.ToSlash(rel)
		}
	}
	return displayPath(p)
}

func (a *app) drawEditor(b uv.ScreenBuffer) {
	x0, y0 := a.editorX(), a.editorY()
	w, h := a.editorW(), a.editorH()

	vls := a.layoutEditor()
	curIdx, _ := a.cursorVisual(vls)

	// scroll to keep the cursor's visual row on screen.
	scroll := a.scroll
	if curIdx < scroll {
		scroll = curIdx
	}
	if curIdx >= scroll+h {
		scroll = curIdx - h + 1
	}
	if scroll < 0 {
		scroll = 0
	}
	a.scroll = scroll

	selA, selB, hasSel := a.ed.selRange()
	isTarget := a.lib.isTarget

	for sy := 0; sy < h; sy++ {
		vi := scroll + sy
		if vi >= len(vls) {
			break
		}
		vl := vls[vi]
		line := a.ed.lines[vl.row]
		seg := line[vl.start:vl.end]
		base := lineBaseStyle(string(line))

		// draw glyphs
		for i, rn := range seg {
			col := vl.start + i
			stl := base
			if hasSel && inSel(vl.row, col, selA, selB) {
				stl = tst(colSelFG, colSelBG, 0)
			}
			cell(b, x0+i, y0+sy, glyph(rn), stl)
		}
		// selection extension past end-of-line (show a thin marker)
		if hasSel && vl.end == len(line) && selSpansLineEnd(vl.row, selA, selB) {
			if len(seg) < w {
				cell(b, x0+len(seg), y0+sy, " ", tst(colSelFG, colSelBG, 0))
			}
		}
		// overlay link styling (skip frontmatter/fence lines)
		if !inFrontmatterOrFence(a.ed, vl.row) {
			a.styleLinks(b, x0, y0+sy, vl, line, isTarget, hasSel, selA, selB)
		}
	}

	// overlay find matches (the active one is already the selection).
	if a.fbar.active {
		for i, m := range a.fbar.matches {
			if i == a.fbar.idx {
				continue
			}
			for col := m.start; col < m.end; col++ {
				if sx, sy, ok := a.cellScreen(m.row, col, vls); ok {
					orAttr(b, sx, sy, uv.AttrReverse)
				}
			}
		}
	}
}

func (a *app) styleLinks(b uv.ScreenBuffer, x0, y int, vl visualLine, line []rune, isTarget func(string) bool, hasSel bool, selA, selB pos) {
	for _, sp := range scanLine(line, isTarget) {
		var stl uv.Style
		switch sp.kind {
		case linkOK:
			stl = tstUnder(colOK)
		case linkBad:
			stl = tst(colBad, nil, 0)
		case linkFuzzyKind:
			stl = tstUnder(colFuzzy)
		case linkMarkdown:
			stl = tstUnder(colAccent)
		}
		for col := sp.start; col < sp.end && col < vl.end; col++ {
			if col < vl.start {
				continue
			}
			if hasSel && inSel(vl.row, col, selA, selB) {
				continue
			}
			cell(b, x0+(col-vl.start), y, glyph(line[col]), stl)
		}
	}
}

func lineBaseStyle(s string) uv.Style {
	switch {
	case strings.HasPrefix(s, "#"):
		return tst(colHeading, nil, uv.AttrBold)
	case strings.HasPrefix(s, ">"):
		return tst(colMuted, nil, uv.AttrItalic)
	case strings.HasPrefix(s, "```") || strings.HasPrefix(s, "~~~"):
		return tst(colMuted, nil, 0)
	}
	return tst(nil, nil, 0)
}

func inSel(row, col int, a, b pos) bool {
	p := pos{row, col}
	return !posLess(p, a) && posLess(p, b)
}

func selSpansLineEnd(row int, a, b pos) bool {
	// selection continues onto a following line from this row
	return row >= a.row && row < b.row
}

func (a *app) cursorScreen() (int, int, bool) {
	vls := a.layoutEditor()
	curIdx, vcol := a.cursorVisual(vls)
	sy := curIdx - a.scroll
	if sy < 0 || sy >= a.editorH() {
		return 0, 0, false
	}
	return a.editorX() + vcol, a.editorY() + sy, true
}

func (a *app) drawStatus(b uv.ScreenBuffer) {
	y := a.rows - 1
	st := tst(colMuted, colBarBG, 0)
	fill(b, 0, y, a.cols, 1, " ", st)

	left := a.status
	if left == "" {
		left = a.defaultHint()
	}

	// right: word count · reading time · line:col
	words, mins := a.ed.wordStats()
	right := fmt.Sprintf("%d words", words)
	if words >= 100 {
		right += fmt.Sprintf(" · %d min", mins)
	}
	right += fmt.Sprintf("   %d:%d", a.ed.cur.row+1, a.ed.cur.col+1)

	textClip(b, 1, y, a.cols-len(right)-3, left, st)
	text(b, a.cols-len(right)-1, y, right, tst(colMuted, colBarBG, uv.AttrBold))
}

func (a *app) defaultHint() string {
	return "ctrl+k files · ctrl+f find · ctrl+g grab · ctrl+l link · ctrl+s save · ctrl+q quit"
}

func (a *app) drawFinder(b uv.ScreenBuffer) {
	w := a.cols * 3 / 4
	if w > 88 {
		w = 88
	}
	if w < 24 {
		w = a.cols - 2
	}
	x := (a.cols - w) / 2
	maxRows := a.rows - 6
	if maxRows < 3 {
		maxRows = 3
	}
	items := len(a.fnd.hits)
	if a.fnd.canCreate {
		items++
	}
	shown := items
	if shown > maxRows {
		shown = maxRows
	}
	h := shown + 3
	y := 2

	faint(b, 0, 1, a.cols, a.rows-2)
	fill(b, x, y, w, h, " ", tst(nil, nil, 0))
	frame(b, x, y, w, h, boxRound, tst(colAccent, nil, 0))

	// query line
	text(b, x+2, y+1, "> "+string(a.fnd.query), tst(nil, nil, uv.AttrBold))
	if a.fnd.total > len(a.fnd.hits) {
		cnt := fmt.Sprintf("%d of %d", len(a.fnd.hits), a.fnd.total)
		text(b, x+w-len(cnt)-2, y+1, cnt, tst(colMuted, nil, 0))
	}

	// scroll window around selection
	top := 0
	if a.fnd.sel >= shown {
		top = a.fnd.sel - shown + 1
	}
	for i := 0; i < shown; i++ {
		idx := top + i
		ry := y + 2 + i
		selected := idx == a.fnd.sel
		rowStyle := tst(nil, nil, 0)
		if selected {
			fill(b, x+1, ry, w-2, 1, " ", tst(colSelFG, colSelBG, 0))
			rowStyle = tst(colSelFG, colSelBG, 0)
		}
		if idx < len(a.fnd.hits) {
			hit := a.fnd.hits[idx]
			name := strings.TrimSuffix(hit.name, ".jd")
			text(b, x+2, ry, name, rowStyle)
			if hit.dir != "" {
				dirStyle := tst(colMuted, nil, 0)
				if selected {
					dirStyle = rowStyle
				}
				text(b, x+w-len([]rune(hit.dir))-2, ry, hit.dir, dirStyle)
			}
		} else {
			// create row
			label := "+ Create \"" + a.fnd.createName + ".jd\""
			text(b, x+2, ry, label, mergeStyle(rowStyle, colOK))
		}
	}
	if items == 0 {
		text(b, x+2, y+2, "no matches", tst(colMuted, nil, 0))
	}
}

func mergeStyle(base uv.Style, fg color.Color) uv.Style {
	if base.Bg != nil {
		return base
	}
	return tst(fg, nil, 0)
}

func (a *app) drawPopup(b uv.ScreenBuffer) {
	// anchor below the token on its visual row.
	vls := a.layoutEditor()
	// find visual row for the token position
	anchorSY := -1
	anchorCol := 0
	for i, vl := range vls {
		if vl.row == a.pop.tokRow && a.pop.tokStart >= vl.start && a.pop.tokStart < vl.end {
			anchorSY = i - a.scroll
			anchorCol = a.pop.tokStart - vl.start
			break
		}
		if vl.row == a.pop.tokRow && i+1 <= len(vls) {
			anchorSY = i - a.scroll
			anchorCol = a.pop.tokStart - vl.start
		}
	}
	if anchorSY < 0 {
		anchorSY = 0
	}
	px := a.editorX() + anchorCol
	py := a.editorY() + anchorSY + 1

	n := len(a.pop.matches)
	if n > 8 {
		n = 8
	}
	w := 0
	for _, m := range a.pop.matches[:n] {
		if l := len(justdown.Leaf(m.Key)) + len(m.Key) + len(m.Kind) + 6; l > w {
			w = l
		}
	}
	if w < 24 {
		w = 24
	}
	if w > a.cols-4 {
		w = a.cols - 4
	}
	if px+w > a.cols {
		px = a.cols - w
	}
	if px < 0 {
		px = 0
	}
	h := n + 2
	if py+h > a.rows-1 {
		py = a.editorY() + anchorSY - h // flip above
	}
	if py < 1 {
		py = 1
	}

	fill(b, px, py, w, h, " ", tst(nil, colBarBG, 0))
	frame(b, px, py, w, h, boxRound, tst(colAccent, colBarBG, 0))
	header := "@ link"
	if a.pop.fuzzy {
		header = "@? fuzzy"
	}
	text(b, px+1, py, header, tst(colMuted, colBarBG, 0))

	for i := 0; i < n; i++ {
		m := a.pop.matches[i]
		ry := py + 1 + i
		rowStyle := tst(nil, colBarBG, 0)
		if i == a.pop.sel {
			fill(b, px+1, ry, w-2, 1, " ", tst(colSelFG, colSelBG, 0))
			rowStyle = tst(colSelFG, colSelBG, 0)
		}
		leaf := justdown.Leaf(m.Key)
		leafColor := colOK
		if !m.Source.IsLocal() {
			leafColor = colRemote // remote capability — read-only
		}
		nx := text(b, px+1, ry, leaf, mergeStyle(rowStyle, leafColor))
		text(b, nx+1, ry, m.Key, dimIfNotSel(rowStyle))
		if m.Kind != "" {
			kind := "[" + m.Kind + "]"
			text(b, px+w-len(kind)-1, ry, kind, dimIfNotSel(rowStyle))
		}
	}
}

func dimIfNotSel(rowStyle uv.Style) uv.Style {
	if rowStyle.Bg != nil {
		return rowStyle
	}
	return tst(colMuted, colBarBG, 0)
}

func (a *app) drawRg(b uv.ScreenBuffer) {
	r := &a.rg
	h := 12
	if h > a.rows/2 {
		h = a.rows / 2
	}
	if h < 4 {
		h = 4
	}
	y := a.rows - 1 - h
	w := a.cols
	x := 0

	fill(b, x, y, w, h, " ", tst(nil, colBarBG, 0))
	frame(b, x, y, w, h, boxRound, tst(colAccent, colBarBG, 0))

	// query line
	text(b, x+2, y, " grab: search all .jd ", tst(colMuted, colBarBG, 0))
	text(b, x+2, y+1, "> "+string(r.query), tst(nil, colBarBG, uv.AttrBold))
	cnt := fmt.Sprintf("%d hits", len(r.hits))
	text(b, x+w-len(cnt)-2, y+1, cnt, tst(colMuted, colBarBG, 0))

	rowsAvail := h - 3
	if rowsAvail < 1 {
		return
	}
	top := 0
	if r.sel >= rowsAvail {
		top = r.sel - rowsAvail + 1
	}
	for i := 0; i < rowsAvail; i++ {
		idx := top + i
		if idx >= len(r.hits) {
			break
		}
		ry := y + 2 + i
		hit := r.hits[idx]
		rowStyle := tst(nil, colBarBG, 0)
		if idx == r.sel {
			fill(b, x+1, ry, w-2, 1, " ", tst(colSelFG, colSelBG, 0))
			rowStyle = tst(colSelFG, colSelBG, 0)
		}
		loc := fmt.Sprintf("%s:%d", a.titleFor(hit.path), hit.line)
		nx := text(b, x+2, ry, loc, mergeStyle(rowStyle, colAccent))
		text(b, nx+2, ry, hit.text, dimIfNotSel(rowStyle))
	}
	if len(r.hits) == 0 && len(r.query) > 0 {
		text(b, x+2, y+3, "no matches", tst(colMuted, colBarBG, 0))
	}
}

func (a *app) drawFindBar(b uv.ScreenBuffer) {
	f := &a.fbar
	w := 44
	if w > a.cols-2 {
		w = a.cols - 2
	}
	x := a.cols - w - 1
	if x < 0 {
		x = 0
	}
	y := 1
	h := 3
	if f.withReplace {
		h = 4
	}
	fill(b, x, y, w, h, " ", tst(nil, colBarBG, 0))
	frame(b, x, y, w, h, boxRound, tst(colAccent, colBarBG, 0))

	// find row
	findLabel := "find "
	labelStyle := tst(colMuted, colBarBG, 0)
	if !f.focusRepl {
		labelStyle = tst(colAccent, colBarBG, uv.AttrBold)
	}
	nx := text(b, x+1, y+1, findLabel, labelStyle)
	textClip(b, nx, y+1, w-16, string(f.query), tst(nil, colBarBG, 0))
	count := "0/0"
	if len(f.matches) > 0 {
		count = fmt.Sprintf("%d/%d", f.idx+1, len(f.matches))
	}
	aa := "aa"
	if f.caseSens {
		aa = "Aa"
	}
	meta := count + " " + aa
	text(b, x+w-len(meta)-1, y+1, meta, tst(colMuted, colBarBG, 0))

	// replace row
	if !f.withReplace {
		return
	}
	replLabel := "repl "
	replStyle := tst(colMuted, colBarBG, 0)
	if f.focusRepl {
		replStyle = tst(colAccent, colBarBG, uv.AttrBold)
	}
	nx = text(b, x+1, y+2, replLabel, replStyle)
	textClip(b, nx, y+2, w-8, string(f.repl), tst(nil, colBarBG, 0))
}

func (a *app) cellScreen(row, col int, vls []visualLine) (int, int, bool) {
	for i, vl := range vls {
		if vl.row != row {
			continue
		}
		last := i+1 >= len(vls) || vls[i+1].row != row
		if col >= vl.start && (col < vl.end || (last && col <= vl.end)) {
			sy := i - a.scroll
			if sy < 0 || sy >= a.editorH() {
				return 0, 0, false
			}
			return a.editorX() + (col - vl.start), a.editorY() + sy, true
		}
	}
	return 0, 0, false
}

func (a *app) drawConfirm(b uv.ScreenBuffer) {
	msg := "Delete the whole file? "
	w := len(msg) + 12
	x := (a.cols - w) / 2
	y := a.rows / 2
	fill(b, x, y, w, 3, " ", tst(nil, nil, 0))
	frame(b, x, y, w, 3, boxRound, tst(colBad, nil, 0))
	nx := text(b, x+2, y+1, msg, tst(nil, nil, uv.AttrBold))
	nx = text(b, nx, y+1, "y", tst(colBad, nil, uv.AttrBold))
	nx = text(b, nx, y+1, "/", tst(colMuted, nil, 0))
	text(b, nx, y+1, "n", tst(colOK, nil, uv.AttrBold))
}
