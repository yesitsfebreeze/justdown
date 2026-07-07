package editor

import (
	"strings"
	"testing"

	uv "github.com/charmbracelet/ultraviolet"
)

func noTargets(string) bool { return false }

func TestClassifyLines(t *testing.T) {
	doc := "---\nname: x\n---\n\n# Head\n\n> quote\n\n| a | b |\n| - | - |\n| c | d |\n\n```\ncode\n```\n---\ntext"
	lines := splitLines(doc)
	infos := classifyLines(lines)
	want := []lineKind{
		lkFmDelim, lkFmLine, lkFmDelim, lkEmpty, lkHeading, lkEmpty,
		lkQuote, lkEmpty, lkTableRow, lkTableSep, lkTableRow, lkEmpty,
		lkFence, lkCode, lkFence, lkHR, lkProse,
	}
	for i, w := range want {
		if infos[i].kind != w {
			t.Errorf("line %d (%q): kind %d want %d", i, string(lines[i]), infos[i].kind, w)
		}
	}
	if !infos[8].header {
		t.Errorf("first table row should be marked header")
	}
}

func TestBlockAt(t *testing.T) {
	doc := "# Head\n\npara one\npara two\n\n- item a\n- item b\n\n| a |\n| - |\n| b |"
	lines := splitLines(doc)
	infos := classifyLines(lines)
	cases := []struct{ row, start, end int }{
		{0, 0, 0},   // heading is its own block
		{2, 2, 3},   // paragraph spans both lines
		{5, 5, 5},   // each list item is its own block
		{6, 6, 6},
		{9, 8, 10},  // separator selects the whole table
	}
	for _, c := range cases {
		s, e := blockAt(lines, infos, c.row)
		if s != c.start || e != c.end {
			t.Errorf("blockAt(%d) = %d..%d, want %d..%d", c.row, s, e, c.start, c.end)
		}
	}
}

func TestInlineSpans(t *testing.T) {
	line := []rune("a **b** *i* ~~s~~ `c` ***bi*** snake_case")
	spans := inlineSpans(line, 0, len(line))
	var bold, italic, strike, code, both int
	for _, sp := range spans {
		switch {
		case sp.code:
			code++
		case sp.attrs == uv.AttrBold:
			bold++
		case sp.attrs == uv.AttrItalic:
			italic++
		case sp.attrs == uv.AttrStrikethrough:
			strike++
		case sp.attrs == uv.AttrBold|uv.AttrItalic:
			both++
		}
	}
	if bold != 1 || italic != 1 || strike != 1 || code != 1 || both != 1 {
		t.Fatalf("spans: bold=%d italic=%d strike=%d code=%d both=%d (%v)", bold, italic, strike, code, both, spans)
	}
}

func TestInlineSpansNested(t *testing.T) {
	line := []rune("**bold *inner* bold**")
	spans := inlineSpans(line, 0, len(line))
	if len(spans) != 2 {
		t.Fatalf("want outer+inner spans, got %v", spans)
	}
	inner := spans[1]
	if inner.attrs != uv.AttrItalic {
		t.Fatalf("inner span should be italic: %v", inner)
	}
}

func TestRenderLineConcealsMarkers(t *testing.T) {
	line := []rune("**bo**")
	info := lineInfo{kind: lkProse}
	inactive := renderLine(line, info, false, noTargets)
	if len(inactive) != 2 {
		t.Fatalf("inactive: want 2 visible cells, got %d (%v)", len(inactive), inactive)
	}
	for _, rc := range inactive {
		if rc.st.Attrs&uv.AttrBold == 0 {
			t.Fatalf("content should be bold: %+v", rc)
		}
	}
	active := renderLine(line, info, true, noTargets)
	if len(active) != len(line) {
		t.Fatalf("active line must stay 1:1: got %d cells for %d runes", len(active), len(line))
	}
	for i, rc := range active {
		if rc.src != i {
			t.Fatalf("active cell %d has src %d", i, rc.src)
		}
	}
}

func TestRenderLineHeading(t *testing.T) {
	line := []rune("## Title")
	cells := renderLine(line, lineInfo{kind: lkHeading}, false, noTargets)
	if len(cells) != 5 { // "Title", "## " concealed
		t.Fatalf("heading conceal: want 5 cells, got %d (%v)", len(cells), cells)
	}
	if cells[0].g != "T" || cells[0].st.Attrs&uv.AttrBold == 0 {
		t.Fatalf("heading content wrong: %+v", cells[0])
	}
}

func TestRenderLineTable(t *testing.T) {
	line := []rune("| a | b |")
	cells := renderLine(line, lineInfo{kind: lkTableRow, header: true}, false, noTargets)
	if len(cells) != len(line) {
		t.Fatalf("table row keeps geometry: got %d cells", len(cells))
	}
	if cells[0].g != "│" {
		t.Fatalf("pipe should render as │, got %q", cells[0].g)
	}
	if cells[2].st.Attrs&uv.AttrBold == 0 {
		t.Fatalf("header cell should be bold: %+v", cells[2])
	}
	// active (cursor inside): raw pipes for editing
	raw := renderLine(line, lineInfo{kind: lkTableRow, header: true}, true, noTargets)
	if raw[0].g != "|" {
		t.Fatalf("active table row must show raw '|', got %q", raw[0].g)
	}
}

func TestRenderLineMdLink(t *testing.T) {
	line := []rune("see [here](./x.jd) ok")
	cells := renderLine(line, lineInfo{kind: lkProse}, false, noTargets)
	var visible strings.Builder
	for _, rc := range cells {
		visible.WriteString(rc.g)
	}
	if visible.String() != "see here ok" {
		t.Fatalf("md link conceal: got %q", visible.String())
	}
}

func TestRenderLineBulletAndCheckbox(t *testing.T) {
	cells := renderLine([]rune("- [x] done"), lineInfo{kind: lkProse}, false, noTargets)
	var visible strings.Builder
	for _, rc := range cells {
		visible.WriteString(rc.g)
	}
	if visible.String() != "• ☑ done" {
		t.Fatalf("checkbox render: got %q", visible.String())
	}
	last := cells[len(cells)-1]
	if last.st.Attrs&uv.AttrStrikethrough == 0 {
		t.Fatalf("checked item content should be struck through: %+v", last)
	}
}
