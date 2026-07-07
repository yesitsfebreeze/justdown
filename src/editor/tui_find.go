package editor

import (
	"strings"
	"unicode"
)

type matchRange struct {
	row   int
	start int
	end   int
}

// findBar is the in-document find overlay (ctrl+f); ctrl+shift+f (or Tab, on
// terminals that can't report shift) adds the replace field. ctrl+r replaces
// the current match, ctrl+a replaces all.
type findBar struct {
	active      bool
	withReplace bool
	query       []rune
	repl        []rune
	focusRepl   bool
	caseSens    bool
	matches     []matchRange
	idx         int
}

func (a *app) openFind() {
	a.pop.close()
	f := &a.fbar
	f.active = true
	f.withReplace = false
	f.focusRepl = false
	if sel := a.ed.selectedText(); sel != "" && !strings.Contains(sel, "\n") {
		f.query = []rune(sel)
	}
	a.recomputeMatches()
	a.findFromCursor()
}

func (a *app) openReplace() {
	a.openFind()
	a.fbar.withReplace = true
}

func (a *app) closeFind() {
	a.fbar.active = false
	a.fbar.matches = nil
}

// matchesInLine returns the [start,end) rune-column spans of every occurrence
// of needle in line. It matches rune-by-rune over the original line, so the
// returned columns are always valid indices into line even when case-folding
// changes a rune's byte or rune count (e.g. İ → i).
func matchesInLine(line []rune, needle string, caseSens bool) [][2]int {
	nr := []rune(needle)
	if len(nr) == 0 || len(nr) > len(line) {
		return nil
	}
	fold := func(r rune) rune {
		if caseSens {
			return r
		}
		return unicode.ToLower(r)
	}
	var out [][2]int
	i := 0
	for i+len(nr) <= len(line) {
		match := true
		for j := 0; j < len(nr); j++ {
			if fold(line[i+j]) != fold(nr[j]) {
				match = false
				break
			}
		}
		if match {
			out = append(out, [2]int{i, i + len(nr)})
			i += len(nr)
		} else {
			i++
		}
	}
	return out
}

func (a *app) recomputeMatches() {
	f := &a.fbar
	f.matches = nil
	needle := string(f.query)
	if needle == "" {
		return
	}
	for r, line := range a.ed.lines {
		for _, m := range matchesInLine(line, needle, f.caseSens) {
			f.matches = append(f.matches, matchRange{row: r, start: m[0], end: m[1]})
		}
	}
	if f.idx >= len(f.matches) {
		f.idx = 0
	}
}

// findFromCursor selects the first match at or after the cursor.
func (a *app) findFromCursor() {
	f := &a.fbar
	if len(f.matches) == 0 {
		return
	}
	cur := a.ed.cur
	for i, m := range f.matches {
		if m.row > cur.row || (m.row == cur.row && m.start >= cur.col) {
			f.idx = i
			a.gotoMatch()
			return
		}
	}
	f.idx = 0
	a.gotoMatch()
}

func (a *app) gotoMatch() {
	f := &a.fbar
	if f.idx < 0 || f.idx >= len(f.matches) {
		return
	}
	m := f.matches[f.idx]
	a.ed.anchor = pos{row: m.row, col: m.start}
	a.ed.cur = pos{row: m.row, col: m.end}
	a.ed.selecting = true
	a.ed.goalCol = m.end
}

func (a *app) findNext() {
	f := &a.fbar
	if len(f.matches) == 0 {
		return
	}
	f.idx = (f.idx + 1) % len(f.matches)
	a.gotoMatch()
}

func (a *app) findPrev() {
	f := &a.fbar
	if len(f.matches) == 0 {
		return
	}
	f.idx = (f.idx - 1 + len(f.matches)) % len(f.matches)
	a.gotoMatch()
}

func (a *app) replaceCurrent() {
	f := &a.fbar
	if f.idx < 0 || f.idx >= len(f.matches) {
		return
	}
	m := f.matches[f.idx]
	a.ed.pushUndo("")
	a.ed.lastKind = ""
	line := a.ed.lines[m.row]
	repl := append([]rune(nil), f.repl...)
	nl := make([]rune, 0, len(line)-(m.end-m.start)+len(repl))
	nl = append(nl, line[:m.start]...)
	nl = append(nl, repl...)
	nl = append(nl, line[m.end:]...)
	a.ed.lines[m.row] = nl
	a.ed.cur = pos{row: m.row, col: m.start + len(repl)}
	a.ed.selecting = false
	a.ed.markDirty()
	a.recomputeMatches()
	a.findFromCursor()
}

func (a *app) replaceAll() {
	f := &a.fbar
	if len(f.matches) == 0 {
		return
	}
	a.ed.pushUndo("")
	a.ed.lastKind = ""
	repl := string(f.repl)
	// replace right-to-left so earlier match positions stay valid.
	for i := len(f.matches) - 1; i >= 0; i-- {
		m := f.matches[i]
		line := a.ed.lines[m.row]
		nl := make([]rune, 0, len(line))
		nl = append(nl, line[:m.start]...)
		nl = append(nl, []rune(repl)...)
		nl = append(nl, line[m.end:]...)
		a.ed.lines[m.row] = nl
	}
	n := len(f.matches)
	a.ed.selecting = false
	a.ed.clampCursor()
	a.ed.markDirty()
	a.recomputeMatches()
	a.setStatus("replaced " + itoa(n))
}

func (a *app) keyFind(k key) {
	f := &a.fbar
	if k.kind == kRune && k.ctrl {
		switch k.r {
		case 'r':
			a.replaceCurrent()
		case 'a':
			a.replaceAll()
		case 'i':
			f.caseSens = !f.caseSens
			a.recomputeMatches()
			a.findFromCursor()
		case 'f':
			if k.shift {
				f.withReplace = true
				f.focusRepl = true
			} else {
				a.closeFind()
			}
		case 'n':
			a.findNext()
		case 'p':
			a.findPrev()
		}
		return
	}
	switch k.kind {
	case kEsc:
		a.closeFind()
	case kEnter:
		if f.focusRepl {
			a.replaceCurrent()
		} else {
			a.findNext()
		}
	case kTab, kBacktab:
		f.withReplace = true
		f.focusRepl = !f.focusRepl
	case kDown:
		a.findNext()
	case kUp:
		a.findPrev()
	case kBackspace:
		if f.focusRepl {
			if len(f.repl) > 0 {
				f.repl = f.repl[:len(f.repl)-1]
			}
		} else if len(f.query) > 0 {
			f.query = f.query[:len(f.query)-1]
			a.recomputeMatches()
			a.findFromCursor()
		}
	case kRune:
		if f.focusRepl {
			f.repl = append(f.repl, k.r)
		} else {
			f.query = append(f.query, k.r)
			a.recomputeMatches()
			a.findFromCursor()
		}
	}
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	neg := n < 0
	if neg {
		n = -n
	}
	var b [20]byte
	i := len(b)
	for n > 0 {
		i--
		b[i] = byte('0' + n%10)
		n /= 10
	}
	if neg {
		i--
		b[i] = '-'
	}
	return string(b[i:])
}

// wordStats counts body words (frontmatter and fenced code excluded) and the
// reading time in minutes at ~200 wpm.
func (e *editor) wordStats() (words, minutes int) {
	inFM := false
	inFence := false
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
		if strings.HasPrefix(s, "```") || strings.HasPrefix(s, "~~~") {
			inFence = !inFence
			continue
		}
		if inFence {
			continue
		}
		words += len(strings.Fields(s))
	}
	minutes = (words + 199) / 200
	return words, minutes
}
