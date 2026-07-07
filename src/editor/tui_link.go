package editor

import (
	"regexp"

	justdown "github.com/yesitsfebreeze/justdown/src"
)

// caret-token regex: @, optional ?, then a name or dir/name of link chars.
var jdTokenRE = regexp.MustCompile(`@(\??)([a-z0-9_-]*(?:/[a-z0-9_-]*)?)$`)

// inline @link, scanned in prose only (frontmatter / fences excluded upstream).
var jdLinkRE = regexp.MustCompile(`@(\?)?([a-z0-9_-]+(?:/[a-z0-9_-]+)?)`)

// markdown link [text](target)
var mdLinkRE = regexp.MustCompile(`\[[^\]]*\]\(([^)]+)\)`)

type linkpop struct {
	active  bool
	fuzzy   bool
	needle  string
	matches []*justdown.Row
	sel     int
	// token span on the current line (rune columns), for redraw + rewrite.
	tokRow   int
	tokStart int
	tokEnd   int
}

func (p *linkpop) close() {
	p.active = false
	p.matches = nil
	p.sel = 0
}

func (p *linkpop) move(d int) {
	if len(p.matches) == 0 {
		return
	}
	p.sel = (p.sel + d) % len(p.matches)
	if p.sel < 0 {
		p.sel += len(p.matches)
	}
}

// tokenAtCaret returns the @-token immediately left of the caret, if the caret
// sits in prose (outside frontmatter and fenced code).
func tokenAtCaret(e *editor) (needle string, fuzzy bool, start int, ok bool) {
	if inFrontmatterOrFence(e, e.cur.row) {
		return "", false, 0, false
	}
	line := e.curLine()
	before := string(line[:e.cur.col])
	m := jdTokenRE.FindStringSubmatch(before)
	if m == nil {
		return "", false, 0, false
	}
	fuzzy = m[1] == "?"
	needle = m[2]
	if needle == "" {
		return "", false, 0, false
	}
	// start column = caret minus the matched token length (including @ and ?).
	start = e.cur.col - len([]rune(m[0]))
	return needle, fuzzy, start, true
}

// inFrontmatterOrFence reports whether the given line index is inside the
// leading YAML frontmatter block or a fenced code block.
func inFrontmatterOrFence(e *editor, row int) bool {
	inFM := false
	inFence := false
	for i := 0; i <= row && i < len(e.lines); i++ {
		s := string(e.lines[i])
		if i == 0 && s == "---" {
			inFM = true
			continue
		}
		if inFM {
			if s == "---" {
				inFM = false
			}
			if i == row {
				return true
			}
			continue
		}
		if len(s) >= 3 && (s[:3] == "```" || s[:3] == "~~~") {
			if i == row {
				return true // the fence line itself renders raw
			}
			inFence = !inFence
			continue
		}
		if i == row {
			return inFence
		}
	}
	return inFM || inFence
}

type linkSpan struct {
	start int // rune col
	end   int
	kind  linkKind
	token string // without @, without ? for fuzzy
	fuzzy bool
}

type linkKind int

const (
	linkOK linkKind = iota
	linkBad
	linkFuzzyKind
	linkMarkdown
)

// scanLine finds @links and markdown links on a prose line for styling/follow.
func scanLine(line []rune, isTarget func(string) bool) []linkSpan {
	s := string(line)
	var spans []linkSpan

	for _, loc := range jdLinkRE.FindAllStringSubmatchIndex(s, -1) {
		startByte, endByte := loc[0], loc[1]
		fuzzy := loc[2] >= 0
		tokByte := s[loc[4]:loc[5]]
		start := len([]rune(s[:startByte]))
		end := len([]rune(s[:endByte]))
		kind := linkBad
		if fuzzy {
			kind = linkFuzzyKind
		} else if isTarget(tokByte) {
			kind = linkOK
		}
		spans = append(spans, linkSpan{start: start, end: end, kind: kind, token: tokByte, fuzzy: fuzzy})
	}

	for _, loc := range mdLinkRE.FindAllStringSubmatchIndex(s, -1) {
		start := len([]rune(s[:loc[0]]))
		end := len([]rune(s[:loc[1]]))
		target := s[loc[2]:loc[3]]
		spans = append(spans, linkSpan{start: start, end: end, kind: linkMarkdown, token: target})
	}
	return spans
}

// linkAtCaret returns the link span the caret is on, for "follow link".
func linkAtCaret(e *editor, isTarget func(string) bool) (linkSpan, bool) {
	if inFrontmatterOrFence(e, e.cur.row) {
		return linkSpan{}, false
	}
	for _, sp := range scanLine(e.curLine(), isTarget) {
		if e.cur.col >= sp.start && e.cur.col <= sp.end {
			return sp, true
		}
	}
	return linkSpan{}, false
}
