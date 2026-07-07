package editor

import (
	"image/color"
	"unicode/utf8"

	"charm.land/lipgloss/v2"
	uv "github.com/charmbracelet/ultraviolet"
)

// palette — kept deliberately small; the terminal's own theme carries the rest.
var (
	colAccent  = lipgloss.Color("12") // links / active
	colOK      = lipgloss.Color("10") // resolved @link
	colBad     = lipgloss.Color("9")  // unresolved @link
	colFuzzy   = lipgloss.Color("13") // @?fuzzy link
	colMuted   = lipgloss.Color("8")
	colSelFG   = lipgloss.Color("0")
	colSelBG   = lipgloss.Color("6")
	colBarBG   = lipgloss.Color("0")
	colHeading = lipgloss.Color("14")
	colDirty   = lipgloss.Color("11")
	colRemote  = lipgloss.Color("5")
)

func tst(fg, bg color.Color, attrs uint8) uv.Style {
	return uv.Style{Fg: fg, Bg: bg, Attrs: attrs}
}

func tstUnder(fg color.Color) uv.Style {
	return uv.Style{Fg: fg, Underline: uv.UnderlineSingle}
}

const tuiASCII = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~"

func glyph(r rune) string {
	if r >= ' ' && r < 0x7f {
		i := int(r - ' ')
		return tuiASCII[i : i+1]
	}
	return string(r)
}

func cell(b uv.ScreenBuffer, x, y int, g string, s uv.Style) {
	if x < 0 || y < 0 || x >= b.Width() || y >= b.Height() {
		return
	}
	c := uv.Cell{Content: g, Style: s, Width: 1}
	b.SetCell(x, y, &c)
}

func fill(b uv.ScreenBuffer, x, y, w, h int, g string, s uv.Style) {
	for yy := y; yy < y+h; yy++ {
		for xx := x; xx < x+w; xx++ {
			cell(b, xx, yy, g, s)
		}
	}
}

func text(b uv.ScreenBuffer, x, y int, str string, s uv.Style) int {
	cx := x
	for i, r := range str {
		cell(b, cx, y, str[i:i+utf8.RuneLen(r)], s)
		cx++
	}
	return cx
}

func textClip(b uv.ScreenBuffer, x, y, maxW int, str string, s uv.Style) int {
	if maxW <= 0 {
		return x
	}
	cx, n := x, 0
	for i, r := range str {
		if n >= maxW {
			break
		}
		cell(b, cx, y, str[i:i+utf8.RuneLen(r)], s)
		cx++
		n++
	}
	return cx
}

func runes(b uv.ScreenBuffer, x, y int, rs []rune, s uv.Style) {
	cx := x
	for _, r := range rs {
		cell(b, cx, y, glyph(r), s)
		cx++
	}
}

func orAttr(b uv.ScreenBuffer, x, y int, attrs uint8) {
	c := b.CellAt(x, y)
	if c == nil || c.Style.Attrs&attrs == attrs {
		return
	}
	cc := *c
	cc.Style.Attrs |= attrs
	b.SetCell(x, y, &cc)
}

func faint(b uv.ScreenBuffer, x, y, w, h int) {
	for yy := y; yy < y+h; yy++ {
		for xx := x; xx < x+w; xx++ {
			orAttr(b, xx, yy, uv.AttrFaint)
		}
	}
}

// tl, tr, bl, br, h, v
var boxRound = [6]string{"╭", "╮", "╰", "╯", "─", "│"}

func frame(b uv.ScreenBuffer, x, y, w, h int, g [6]string, s uv.Style) {
	if w < 2 || h < 2 {
		return
	}
	for xx := x + 1; xx < x+w-1; xx++ {
		cell(b, xx, y, g[4], s)
		cell(b, xx, y+h-1, g[4], s)
	}
	for yy := y + 1; yy < y+h-1; yy++ {
		cell(b, x, yy, g[5], s)
		cell(b, x+w-1, yy, g[5], s)
	}
	cell(b, x, y, g[0], s)
	cell(b, x+w-1, y, g[1], s)
	cell(b, x, y+h-1, g[2], s)
	cell(b, x+w-1, y+h-1, g[3], s)
}
