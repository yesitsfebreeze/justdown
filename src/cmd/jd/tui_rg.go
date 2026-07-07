package main

import "os"

// ripgrep is the global content-search palette (ctrl+p). Arrowing through the
// results previews each hit live in the editor; Enter keeps it, Esc restores
// the file and cursor you started from.
type ripgrep struct {
	active bool
	query  []rune
	hits   []grepHit
	sel    int

	// full editor snapshot to restore on cancel (preserves unsaved edits).
	savedLines  [][]rune
	savedCur    pos
	savedPath   string
	savedRaw    string
	savedDirty  bool
	savedScroll int
	savedUndo   []snapshot
	savedRedo   []snapshot
}

func (a *app) openRg() {
	a.pop.close()
	r := &a.rg
	r.active = true
	r.query = nil
	r.hits = nil
	r.sel = 0
	r.savedPath = a.ed.path
	r.savedCur = a.ed.cur
	r.savedRaw = a.ed.loadedRaw
	r.savedDirty = a.ed.dirty
	r.savedScroll = a.scroll
	r.savedUndo = a.ed.undo
	r.savedRedo = a.ed.redo
	r.savedLines = make([][]rune, len(a.ed.lines))
	for i, ln := range a.ed.lines {
		r.savedLines[i] = append([]rune(nil), ln...)
	}
}

func (a *app) rgSearch() {
	r := &a.rg
	r.hits = a.lib.grep(string(r.query), 200)
	if r.sel >= len(r.hits) {
		r.sel = 0
	}
	a.rgPreview()
}

// rgPreview loads the selected hit's file and selects the matched text.
func (a *app) rgPreview() {
	r := &a.rg
	if r.sel < 0 || r.sel >= len(r.hits) {
		return
	}
	h := r.hits[r.sel]
	if a.ed.path != h.path {
		b, err := os.ReadFile(h.path)
		if err != nil {
			return
		}
		a.ed.setContent(string(b), h.path)
	}
	row := h.line - 1
	if row < 0 {
		row = 0
	}
	if row >= len(a.ed.lines) {
		row = len(a.ed.lines) - 1
	}
	col := h.col - 1
	if col < 0 {
		col = 0
	}
	line := a.ed.lines[row]
	if col > len(line) {
		col = len(line)
	}
	end := col + len([]rune(string(a.rg.query)))
	if end > len(line) {
		end = len(line)
	}
	a.ed.anchor = pos{row: row, col: col}
	a.ed.cur = pos{row: row, col: end}
	a.ed.selecting = end > col
	a.ed.goalCol = end
}

func (a *app) rgCancel() {
	r := &a.rg
	r.active = false
	// restore the original editor state.
	a.ed.lines = r.savedLines
	a.ed.cur = r.savedCur
	a.ed.path = r.savedPath
	a.ed.loadedRaw = r.savedRaw
	a.ed.dirty = r.savedDirty
	a.ed.selecting = false
	a.ed.undo = r.savedUndo
	a.ed.redo = r.savedRedo
	a.scroll = r.savedScroll
	r.hits = nil
	r.savedLines = nil
}

func (a *app) rgConfirm() {
	r := &a.rg
	// Navigating to a DIFFERENT file with unsaved edits would discard them
	// (single buffer). Keep the user's work: restore and nudge them to save.
	// Same-file jumps are safe — preview never clobbered the buffer.
	if r.savedDirty && a.ed.path != r.savedPath {
		a.rgCancel()
		a.setStatus("unsaved changes kept — save (ctrl+s), then search to navigate")
		return
	}
	r.active = false
	r.hits = nil
	r.savedLines = nil
	r.savedUndo = nil
	r.savedRedo = nil
	a.ed.selecting = false
	// the previewed file stays open as loaded from disk.
}

func (a *app) rgMove(d int) {
	r := &a.rg
	if len(r.hits) == 0 {
		return
	}
	r.sel += d
	if r.sel < 0 {
		r.sel = 0
	}
	if r.sel >= len(r.hits) {
		r.sel = len(r.hits) - 1
	}
	a.rgPreview()
}

func (a *app) keyRg(k key) {
	r := &a.rg
	switch k.kind {
	case kEsc:
		a.rgCancel()
	case kEnter:
		a.rgConfirm()
	case kUp:
		a.rgMove(-1)
	case kDown:
		a.rgMove(1)
	case kBackspace:
		if len(r.query) > 0 {
			r.query = r.query[:len(r.query)-1]
			a.rgSearch()
		}
	case kRune:
		if k.ctrl {
			switch k.r {
			case 'n':
				a.rgMove(1)
			case 'p':
				a.rgMove(-1)
			case 'g':
				a.rgCancel()
			}
			return
		}
		r.query = append(r.query, k.r)
		a.rgSearch()
	}
}
