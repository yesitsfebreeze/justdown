package editor

import (
	"image/color"
	"strings"
	"unicode"

	uv "github.com/charmbracelet/ultraviolet"
)

// Live-preview rendering. Markdown is styled cell by cell so buffer geometry
// never lies about the source: the block under the cursor reveals its syntax
// markers (dimmed) and stays 1:1 with the source, every other block conceals
// markers, substitutes glyphs (bullets, table rules) and fades.

type lineKind uint8

const (
	lkProse lineKind = iota
	lkEmpty
	lkHeading
	lkQuote
	lkHR
	lkFmDelim
	lkFmLine
	lkFence
	lkCode
	lkTableRow
	lkTableSep
)

type lineInfo struct {
	kind   lineKind
	header bool // first table row, when a separator follows
}

func classifyLines(lines [][]rune) []lineInfo {
	infos := make([]lineInfo, len(lines))
	inFM, inFence := false, false
	for i, ln := range lines {
		s := string(ln)
		t := strings.TrimSpace(s)
		switch {
		case i == 0 && s == "---":
			inFM = true
			infos[i].kind = lkFmDelim
		case inFM:
			infos[i].kind = lkFmLine
			if s == "---" {
				inFM = false
				infos[i].kind = lkFmDelim
			}
		case strings.HasPrefix(s, "```") || strings.HasPrefix(s, "~~~"):
			inFence = !inFence
			infos[i].kind = lkFence
		case inFence:
			infos[i].kind = lkCode
		case t == "":
			infos[i].kind = lkEmpty
		case isTableRow(ln):
			infos[i].kind = lkTableRow
		case isHR(t):
			infos[i].kind = lkHR
		case headingLevel(ln) > 0:
			infos[i].kind = lkHeading
		case strings.HasPrefix(t, ">"):
			infos[i].kind = lkQuote
		default:
			infos[i].kind = lkProse
		}
	}
	// within each table block: the separator is canonically the second row,
	// and its presence makes the first row a header.
	for i := 0; i < len(infos); {
		if infos[i].kind != lkTableRow {
			i++
			continue
		}
		start := i
		for i < len(infos) && infos[i].kind == lkTableRow {
			i++
		}
		if start+1 < i && isSeparatorRow(lines[start+1]) {
			infos[start+1].kind = lkTableSep
			infos[start].header = true
		}
	}
	return infos
}

func isHR(t string) bool {
	if len(t) < 3 {
		return false
	}
	c := t[0]
	if c != '-' && c != '*' && c != '_' {
		return false
	}
	for i := 1; i < len(t); i++ {
		if t[i] != c {
			return false
		}
	}
	return true
}

// headingLevel returns 1-6 for "# " style headings, 0 otherwise.
func headingLevel(line []rune) int {
	n := 0
	for n < len(line) && line[n] == '#' {
		n++
	}
	if n == 0 || n > 6 || n >= len(line) || line[n] != ' ' {
		return 0
	}
	return n
}

// bulletMark returns the column of a leading list marker ('-', '*', '+' or
// "N.") and whether the line starts one.
func bulletMark(line []rune) (int, bool) {
	i := 0
	for i < len(line) && (line[i] == ' ' || line[i] == '\t') {
		i++
	}
	if i >= len(line) {
		return 0, false
	}
	switch line[i] {
	case '-', '*', '+':
		if i+1 < len(line) && line[i+1] == ' ' {
			return i, true
		}
	}
	if line[i] >= '0' && line[i] <= '9' {
		j := i
		for j < len(line) && line[j] >= '0' && line[j] <= '9' {
			j++
		}
		if j+1 < len(line) && line[j] == '.' && line[j+1] == ' ' {
			return i, true
		}
	}
	return 0, false
}

// blockAt returns the row bounds of the block enclosing row — the unit that
// stays bright and raw while everything else renders dimmed.
func blockAt(lines [][]rune, infos []lineInfo, row int) (int, int) {
	if row < 0 || row >= len(infos) {
		return row, row
	}
	var group map[lineKind]bool
	switch infos[row].kind {
	case lkFmDelim, lkFmLine:
		group = map[lineKind]bool{lkFmDelim: true, lkFmLine: true}
	case lkFence, lkCode:
		group = map[lineKind]bool{lkFence: true, lkCode: true}
	case lkTableRow, lkTableSep:
		group = map[lineKind]bool{lkTableRow: true, lkTableSep: true}
	case lkQuote:
		group = map[lineKind]bool{lkQuote: true}
	case lkProse:
		// a list item is its own block: bullet line plus continuation lines.
		start, end := row, row
		for start > 0 && infos[start-1].kind == lkProse {
			if _, b := bulletMark(lines[start]); b {
				break
			}
			start--
			if _, b := bulletMark(lines[start]); b {
				break
			}
		}
		for end < len(infos)-1 && infos[end+1].kind == lkProse {
			if _, b := bulletMark(lines[end+1]); b {
				break
			}
			end++
		}
		return start, end
	default: // heading, hr, empty
		return row, row
	}
	start, end := row, row
	for start > 0 && group[infos[start-1].kind] {
		start--
	}
	for end < len(infos)-1 && group[infos[end+1].kind] {
		end++
	}
	return start, end
}

// --- inline spans -----------------------------------------------------------

type ispan struct {
	start, end int // full span, markers included
	mlen       int // marker length on each side
	attrs      uint8
	code       bool
}

func emphAttrs(m int) uint8 {
	switch m {
	case 1:
		return uv.AttrItalic
	case 2:
		return uv.AttrBold
	}
	return uv.AttrBold | uv.AttrItalic
}

// inlineSpans scans [from,to) for emphasis, strikethrough and inline code.
// Nested emphasis recurses; code spans are opaque.
func inlineSpans(line []rune, from, to int) []ispan {
	var out []ispan
	for i := from; i < to; {
		r := line[i]
		if r != '*' && r != '_' && r != '~' && r != '`' {
			i++
			continue
		}
		j := i
		for j < to && line[j] == r {
			j++
		}
		k := j - i
		var sp ispan
		ok := false
		switch {
		case r == '`':
			sp, ok = closeCode(line, i, k, to)
		case r == '~':
			if k == 2 {
				sp, ok = closeEmph(line, i, 2, to, uv.AttrStrikethrough)
			}
		default:
			if k <= 3 {
				sp, ok = closeEmph(line, i, k, to, emphAttrs(k))
				if ok && r == '_' && intrawordEmph(line, sp) {
					ok = false
				}
			}
		}
		if !ok {
			i = j
			continue
		}
		out = append(out, sp)
		if !sp.code {
			out = append(out, inlineSpans(line, sp.start+sp.mlen, sp.end-sp.mlen)...)
		}
		i = sp.end
	}
	return out
}

func closeEmph(line []rune, i, m, to int, attrs uint8) (ispan, bool) {
	r := line[i]
	if i+m >= to || line[i+m] == ' ' {
		return ispan{}, false
	}
	for c := i + m + 1; c+m <= to; {
		if line[c] != r {
			c++
			continue
		}
		run := 0
		for c+run < to && line[c+run] == r {
			run++
		}
		if run >= m && line[c-1] != ' ' {
			return ispan{start: i, end: c + m, mlen: m, attrs: attrs}, true
		}
		c += run
	}
	return ispan{}, false
}

func closeCode(line []rune, i, k, to int) (ispan, bool) {
	for c := i + k; c+k <= to; {
		if line[c] != '`' {
			c++
			continue
		}
		run := 0
		for c+run < to && line[c+run] == '`' {
			run++
		}
		if run == k {
			return ispan{start: i, end: c + k, mlen: k, code: true}, true
		}
		c += run
	}
	return ispan{}, false
}

// underscores inside words (snake_case) are not emphasis.
func intrawordEmph(line []rune, sp ispan) bool {
	alnum := func(r rune) bool {
		return r == '_' || unicode.IsLetter(r) || unicode.IsDigit(r)
	}
	if sp.start > 0 && alnum(line[sp.start-1]) {
		return true
	}
	if sp.end < len(line) && alnum(line[sp.end]) {
		return true
	}
	return false
}

// --- line rendering ---------------------------------------------------------

// rcell is one display cell of a rendered line, tagged with the source rune
// column it came from. Active lines always emit every source rune (1:1).
type rcell struct {
	src int
	g   string
	st  uv.Style
}

func renderLine(line []rune, info lineInfo, active bool, isTarget func(string) bool) []rcell {
	n := len(line)
	st := make([]uv.Style, n)
	hide := make([]bool, n)
	gl := make([]string, n)

	// syntax marker: dimmed on the active block, concealed elsewhere.
	mark := func(i int) {
		st[i] = tst(colMuted, nil, 0)
		if !active {
			hide[i] = true
		}
	}
	// glyph substitution (bullet, quote bar, table rules) on inactive lines.
	sub := func(i int, g string, s uv.Style) {
		st[i] = s
		if !active {
			gl[i] = g
		}
	}
	links := func() { applyLinks(line, st, isTarget, mark) }

	switch info.kind {
	case lkEmpty, lkCode:
	case lkFmDelim, lkFence:
		for i := range st {
			st[i] = tst(colMuted, nil, 0)
		}
	case lkHR:
		for i := range st {
			sub(i, "─", tst(colMuted, nil, 0))
		}
	case lkFmLine:
		for i, r := range line {
			st[i] = tst(colMuted, nil, 0)
			if r == ':' {
				break
			}
		}
	case lkHeading:
		m := headingLevel(line) + 1 // '#'s plus the space
		applyInline(line, st, m, n, tst(colHeading, nil, uv.AttrBold), mark)
		links()
		for i := 0; i < m; i++ {
			mark(i)
		}
	case lkQuote:
		q := 0
		for q < n && line[q] == ' ' {
			q++
		}
		applyInline(line, st, q+1, n, tst(nil, nil, uv.AttrItalic), mark)
		links()
		sub(q, "│", tst(colMuted, nil, 0))
	case lkTableSep:
		for i, r := range line {
			switch r {
			case '|':
				sub(i, "│", tst(colMuted, nil, 0))
			case '-':
				sub(i, "─", tst(colMuted, nil, 0))
			default:
				st[i] = tst(colMuted, nil, 0)
			}
		}
	case lkTableRow:
		var base uv.Style
		if info.header {
			base = tst(nil, nil, uv.AttrBold)
		}
		applyInline(line, st, 0, n, base, mark)
		links()
		for i, r := range line {
			if r == '|' {
				sub(i, "│", tst(colMuted, nil, 0))
			}
		}
	default: // prose, possibly a list item
		from := 0
		var base uv.Style
		if bm, ok := bulletMark(line); ok {
			if line[bm] == '-' || line[bm] == '*' || line[bm] == '+' {
				sub(bm, "•", tst(colMuted, nil, 0))
				from = bm + 2
				if bm+4 < n && line[bm+2] == '[' && line[bm+4] == ']' &&
					(line[bm+3] == ' ' || line[bm+3] == 'x' || line[bm+3] == 'X') {
					checked := line[bm+3] != ' '
					mark(bm + 2)
					mark(bm + 4)
					if checked {
						sub(bm+3, "☑", tst(colOK, nil, 0))
						base = tst(colMuted, nil, uv.AttrStrikethrough)
					} else {
						sub(bm+3, "☐", tst(colMuted, nil, 0))
					}
					from = bm + 5
				}
			} else { // ordered "N."
				j := bm
				for j < n && line[j] != '.' {
					j++
				}
				for i := bm; i <= j; i++ {
					st[i] = tst(colMuted, nil, 0)
				}
				from = j + 1
			}
		}
		applyInline(line, st, from, n, base, mark)
		links()
	}

	out := make([]rcell, 0, n)
	for i, r := range line {
		if hide[i] {
			continue
		}
		g := gl[i]
		if g == "" {
			g = glyph(r)
		}
		out = append(out, rcell{src: i, g: g, st: st[i]})
	}
	return out
}

func applyInline(line []rune, st []uv.Style, from, to int, base uv.Style, mark func(int)) {
	if from > to {
		return
	}
	for i := from; i < to; i++ {
		st[i] = base
	}
	for _, sp := range inlineSpans(line, from, to) {
		for i := sp.start + sp.mlen; i < sp.end-sp.mlen; i++ {
			if sp.code {
				st[i].Fg = colCode
			} else {
				st[i].Attrs |= sp.attrs
			}
		}
		for i := sp.start; i < sp.start+sp.mlen; i++ {
			mark(i)
		}
		for i := sp.end - sp.mlen; i < sp.end; i++ {
			mark(i)
		}
	}
}

// applyLinks styles markdown links (concealing [ ](url) around the text) and
// @links, composing over whatever inline attrs are already set.
func applyLinks(line []rune, st []uv.Style, isTarget func(string) bool, mark func(int)) {
	s := string(line)
	for _, loc := range mdLinkRE.FindAllStringSubmatchIndex(s, -1) {
		start := len([]rune(s[:loc[0]]))
		end := len([]rune(s[:loc[1]]))
		textEnd := len([]rune(s[:loc[2]-2])) // "](" sits before the target
		mark(start)
		for i := start + 1; i < textEnd; i++ {
			st[i].Fg = colAccent
			st[i].Underline = uv.UnderlineSingle
		}
		for i := textEnd; i < end; i++ {
			mark(i)
		}
	}
	for _, sp := range scanLine(line, isTarget) {
		var c color.Color
		under := true
		switch sp.kind {
		case linkOK:
			c = colOK
		case linkBad:
			c, under = colBad, false
		case linkFuzzyKind:
			c = colFuzzy
		default:
			continue
		}
		for i := sp.start; i < sp.end && i < len(st); i++ {
			st[i].Fg = c
			if under {
				st[i].Underline = uv.UnderlineSingle
			}
		}
	}
}
