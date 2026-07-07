package main

import (
	"strings"
	"testing"
)

func newEd(content string) *editor {
	e := newEditor()
	e.width = 40
	e.setContent(content, "test.jd")
	e.cur = pos{}
	return e
}

func TestParseKeysModifiedReports(t *testing.T) {
	cases := []struct {
		in   string
		want key
	}{
		{"\x06", key{kind: kRune, r: 'f', ctrl: true}},                          // legacy ctrl+f
		{"\x1b[102;5u", key{kind: kRune, r: 'f', ctrl: true}},                   // CSI-u ctrl+f
		{"\x1b[102;6u", key{kind: kRune, r: 'f', ctrl: true, shift: true}},      // CSI-u ctrl+shift+f
		{"\x1b[70;6u", key{kind: kRune, r: 'f', ctrl: true, shift: true}},       // CSI-u shifted codepoint
		{"\x1b[27;6;102~", key{kind: kRune, r: 'f', ctrl: true, shift: true}},   // modifyOtherKeys
		{"\x1b[27;6;70~", key{kind: kRune, r: 'f', ctrl: true, shift: true}},    // modifyOtherKeys, shifted
		{"\x1b[102:70;6u", key{kind: kRune, r: 'f', ctrl: true, shift: true}},   // kitty sub-params
		{"\x1b[27u", key{kind: kEsc}},                                           // kitty esc
		{"\x1b[13;5u", key{kind: kEnter, ctrl: true}},                           // ctrl+enter
		{"\x1b[9;2u", key{kind: kBacktab}},                                      // shift+tab as CSI-u
	}
	for _, c := range cases {
		ks := parseKeys([]byte(c.in))
		if len(ks) != 1 || ks[0] != c.want {
			t.Errorf("parseKeys(%q) = %+v, want %+v", c.in, ks, c.want)
		}
	}
}

func TestSplitRoundTrip(t *testing.T) {
	for _, s := range []string{"", "a", "a\nb", "a\nb\n", "line1\nline2\nline3"} {
		e := newEd(s)
		if got := e.text(); got != splitJoin(s) {
			t.Errorf("roundtrip %q: got %q want %q", s, got, splitJoin(s))
		}
	}
}

func TestInsertAndBackspace(t *testing.T) {
	e := newEd("")
	for _, r := range "hello" {
		e.insertRune(r)
	}
	if e.text() != "hello" {
		t.Fatalf("insert: got %q", e.text())
	}
	e.backspace()
	if e.text() != "hell" {
		t.Fatalf("backspace: got %q", e.text())
	}
	if !e.dirty {
		t.Fatalf("should be dirty")
	}
}

func TestNewlineSplitAndJoin(t *testing.T) {
	e := newEd("abcd")
	e.cur = pos{0, 2}
	e.insertNewline()
	if e.text() != "ab\ncd" {
		t.Fatalf("newline split: got %q", e.text())
	}
	// backspace at col 0 of line 1 rejoins
	e.cur = pos{1, 0}
	e.backspace()
	if e.text() != "abcd" {
		t.Fatalf("rejoin: got %q", e.text())
	}
}

func TestSelectionAcrossLines(t *testing.T) {
	e := newEd("hello\nworld\nfoo")
	e.cur = pos{0, 2}
	e.anchor = pos{0, 2}
	e.selecting = true
	e.cur = pos{2, 1} // select "llo\nworld\nf"
	if got := e.selectedText(); got != "llo\nworld\nf" {
		t.Fatalf("selectedText: got %q", got)
	}
	e.deleteSelection()
	if e.text() != "heoo" {
		t.Fatalf("deleteSelection: got %q", e.text())
	}
}

func TestUndoRedoDeepCopy(t *testing.T) {
	e := newEd("abc")
	e.cur = pos{0, 3}
	e.insertRune('d') // snapshot of "abc" pushed
	e.insertRune('e')
	// mutate current line; the undo snapshot must not alias it
	e.doUndo()
	if e.text() != "abc" {
		t.Fatalf("undo: got %q", e.text())
	}
	e.doRedo()
	if e.text() != "abcde" {
		t.Fatalf("redo: got %q", e.text())
	}
}

func TestUndoAfterSelectionDelete(t *testing.T) {
	// regression: backspace/delete over a selection must be undoable.
	e := newEd("hello")
	e.cur = pos{0, 2}
	e.anchor = pos{0, 2}
	e.selecting = true
	e.cur = pos{0, 5} // select "llo"
	e.backspace()
	if e.text() != "he" {
		t.Fatalf("delete selection: got %q", e.text())
	}
	e.doUndo()
	if e.text() != "hello" {
		t.Fatalf("undo must restore deleted selection: got %q", e.text())
	}
}

func TestReplaceAllCursorSafe(t *testing.T) {
	// regression: replace-all with a shorter replacement must not leave the
	// cursor past end-of-line (would panic on the next edit).
	e := newEd("foo foo foo")
	a := &app{ed: e, fnd: &finder{}, pop: &linkpop{}}
	e.cur = pos{0, 11} // end of line
	a.fbar.query = []rune("foo")
	a.recomputeMatches()
	a.fbar.repl = nil // delete
	a.replaceAll()
	if e.cur.col > len(e.lines[e.cur.row]) {
		t.Fatalf("cursor col %d past line len %d", e.cur.col, len(e.lines[e.cur.row]))
	}
	// the next edit must not panic
	e.insertRune('x')
	if e.text() != "  x" {
		t.Fatalf("post-replace edit: got %q", e.text())
	}
}

func TestWrapStarts(t *testing.T) {
	line := []rune("aaaa bbbb cccc")
	starts := wrapStarts(line, 10)
	if len(starts) < 2 {
		t.Fatalf("expected wrap, got %v", starts)
	}
	if starts[0] != 0 {
		t.Fatalf("first start must be 0, got %v", starts)
	}
	// long unbreakable token falls back to hard wrap
	long := []rune("aaaaaaaaaaaaaaaaaaaa")
	s2 := wrapStarts(long, 5)
	if len(s2) != 4 {
		t.Fatalf("hard-wrap 20/5 want 4 segments, got %d (%v)", len(s2), s2)
	}
}

func TestMatchesInLineRuneCols(t *testing.T) {
	line := []rune("héllo the thé")
	got := matchesInLine(line, "the", false)
	if len(got) != 1 {
		t.Fatalf("want 1 case-insensitive match of 'the' (not 'thé'), got %v", got)
	}
	// "the" starts at rune index 6 in "héllo the thé"
	if got[0][0] != 6 || got[0][1] != 9 {
		t.Fatalf("rune cols wrong: %v", got)
	}
}

func TestMatchesInLineCaseFoldNoOverrun(t *testing.T) {
	// İ (U+0130) lowercases to multiple runes in some folds; rune-based matching
	// must return in-range columns and not overrun the original line.
	line := []rune("İstanbul is")
	got := matchesInLine(line, "is", false)
	for _, m := range got {
		if m[0] < 0 || m[1] > len(line) || m[0] > m[1] {
			t.Fatalf("out-of-range span %v for line len %d", m, len(line))
		}
	}
}

func TestReplaceAllMultiRow(t *testing.T) {
	e := newEd("foo foo\nbar foo")
	a := &app{ed: e}
	a.fbar.query = []rune("foo")
	a.recomputeMatches()
	if len(a.fbar.matches) != 3 {
		t.Fatalf("want 3 matches, got %d", len(a.fbar.matches))
	}
	a.fbar.repl = []rune("X")
	a.replaceAll()
	if e.text() != "X X\nbar X" {
		t.Fatalf("replaceAll: got %q", e.text())
	}
}

func TestTableNavAndReformat(t *testing.T) {
	e := newEd("| a | bb |\n| - | - |\n| c | d |")
	// cursor in first data cell
	e.cur = pos{2, 2}
	if !e.inTable() {
		t.Fatalf("should be in table")
	}
	e.reformatTable(2)
	// columns pad to max(width,3): col0=3, col1=3 ("bb"->2 but min 3)
	want := "| a   | bb  |"
	if string(e.lines[0]) != want {
		t.Fatalf("reformat header: got %q want %q", string(e.lines[0]), want)
	}
	// Tab from cell 0 -> cell 1 start
	e.cur = pos{2, cellStart(e.lines[2], pipeCols(e.lines[2]), 0)}
	e.tableTab(true)
	c := currentCell(pipeCols(e.lines[2]), e.cur.col)
	if c != 1 {
		t.Fatalf("tableTab should land in cell 1, got %d", c)
	}
}

func TestTableEnterInsertsRow(t *testing.T) {
	e := newEd("| a | b |\n| - | - |\n| c | d |")
	e.cur = pos{2, 2}
	e.tableEnter()
	if len(e.lines) != 4 {
		t.Fatalf("want 4 lines after row insert, got %d", len(e.lines))
	}
	if !isTableRow(e.lines[3]) {
		t.Fatalf("new row should be a table row: %q", string(e.lines[3]))
	}
}

func TestTokenAtCaret(t *testing.T) {
	e := newEd("---\nname: x\n---\n\nsee @bet")
	e.cur = pos{4, 8} // after "see @bet"
	needle, fuzzy, start, ok := tokenAtCaret(e)
	if !ok || needle != "bet" || fuzzy {
		t.Fatalf("tokenAtCaret: ok=%v needle=%q fuzzy=%v start=%d", ok, needle, fuzzy, start)
	}
	// inside frontmatter -> no token
	e.cur = pos{1, 7}
	if _, _, _, ok := tokenAtCaret(e); ok {
		t.Fatalf("should not detect token in frontmatter")
	}
}

func TestScanLineLinks(t *testing.T) {
	targets := map[string]bool{"beta": true}
	isT := func(s string) bool { return targets[strings.ToLower(s)] }
	spans := scanLine([]rune("go to @beta and @nope and [x](./y.jd)"), isT)
	var ok, bad, md int
	for _, s := range spans {
		switch s.kind {
		case linkOK:
			ok++
		case linkBad:
			bad++
		case linkMarkdown:
			md++
		}
	}
	if ok != 1 || bad != 1 || md != 1 {
		t.Fatalf("link classify: ok=%d bad=%d md=%d spans=%v", ok, bad, md, spans)
	}
}

func TestWordStats(t *testing.T) {
	e := newEd("---\nname: x\n---\n\n# Head\n\none two three\n```\ncode ignored\n```\nfour")
	w, _ := e.wordStats()
	// "# Head"(2) + "one two three"(3) + "four"(1) = 6; fence + frontmatter excluded
	if w != 6 {
		t.Fatalf("wordStats: got %d want 6", w)
	}
}
