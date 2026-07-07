package editor

import "unicode/utf8"

type keyKind int

const (
	kRune keyKind = iota
	kEnter
	kTab
	kBacktab
	kBackspace
	kDelete
	kEsc
	kUp
	kDown
	kLeft
	kRight
	kHome
	kEnd
	kPgUp
	kPgDn
	kPasteStart
	kPasteEnd
)

// key is one decoded keystroke. Modifiers are resolved from the raw escape
// sequence: xterm encodes them as 1+(shift=1,alt=2,ctrl=4), so shift is
// (mod-1)&1, alt (mod-1)&2, ctrl (mod-1)&4 — the same decode the burrito TUI uses.
type key struct {
	kind  keyKind
	r     rune
	ctrl  bool
	alt   bool
	shift bool
}

// parseKeys turns a raw stdin chunk into decoded keystrokes. It is tolerant:
// an unrecognised escape sequence is dropped rather than surfaced as garbage.
func parseKeys(data []byte) []key {
	var out []key
	i := 0
	for i < len(data) {
		b := data[i]
		switch {
		case b == 0x1b:
			ev, adv := parseEsc(data[i:])
			if adv == 0 {
				out = append(out, key{kind: kEsc})
				i++
				continue
			}
			if ev.kind != -1 {
				out = append(out, ev.key)
			}
			i += adv
		case b == 0x0d || b == 0x0a:
			out = append(out, key{kind: kEnter})
			i++
		case b == 0x09:
			out = append(out, key{kind: kTab})
			i++
		case b == 0x7f || b == 0x08:
			out = append(out, key{kind: kBackspace})
			i++
		case b == 0x00:
			i++
		case b < 0x20:
			// control byte: ctrl-<letter>. 0x13 == ctrl-s, 0x1a == ctrl-z, etc.
			out = append(out, key{kind: kRune, r: rune('a' + b - 1), ctrl: true})
			i++
		default:
			r, sz := utf8.DecodeRune(data[i:])
			if r == utf8.RuneError && sz <= 1 {
				i++
				continue
			}
			out = append(out, key{kind: kRune, r: r})
			i += sz
		}
	}
	return out
}

type parsedEsc struct {
	kind int // -1 == consumed but no event (e.g. paste markers handled elsewhere)
	key  key
}

// parseEsc decodes one escape sequence at the start of data. It returns the
// event and the number of bytes consumed (0 if data is a lone ESC).
func parseEsc(data []byte) (parsedEsc, int) {
	if len(data) < 2 {
		return parsedEsc{kind: 0, key: key{kind: kEsc}}, 0
	}
	switch data[1] {
	case '[':
		return parseCSI(data)
	case 'O':
		// SS3: arrows / Home / End from application-cursor-key terminals.
		if len(data) < 3 {
			return parsedEsc{kind: 0, key: key{kind: kEsc}}, 0
		}
		if k, ok := finalToArrow(data[2]); ok {
			return parsedEsc{key: key{kind: k}}, 3
		}
		return parsedEsc{kind: -1}, 3
	default:
		// alt-<byte>
		r, sz := utf8.DecodeRune(data[1:])
		if r == utf8.RuneError && sz <= 1 {
			return parsedEsc{kind: 0, key: key{kind: kEsc}}, 1
		}
		return parsedEsc{key: key{kind: kRune, r: r, alt: true}}, 1 + sz
	}
}

func finalToArrow(f byte) (keyKind, bool) {
	switch f {
	case 'A':
		return kUp, true
	case 'B':
		return kDown, true
	case 'C':
		return kRight, true
	case 'D':
		return kLeft, true
	case 'H':
		return kHome, true
	case 'F':
		return kEnd, true
	}
	return 0, false
}

// parseCSI decodes ESC [ ... sequences: arrows, editing keys, and their
// modified forms (ESC [ 1 ; <mod> <final> and ESC [ <n> ; <mod> ~).
func parseCSI(data []byte) (parsedEsc, int) {
	// bracketed paste
	if hasPrefix(data, "\x1b[200~") {
		return parsedEsc{key: key{kind: kPasteStart}}, 6
	}
	if hasPrefix(data, "\x1b[201~") {
		return parsedEsc{key: key{kind: kPasteEnd}}, 6
	}
	if hasPrefix(data, "\x1b[Z") {
		return parsedEsc{key: key{kind: kBacktab}}, 3
	}

	j := 2
	var params []int
	cur := 0
	haveDigit := false
	skipSub := false
	for j < len(data) {
		c := data[j]
		if c >= '0' && c <= '9' {
			if !skipSub {
				cur = cur*10 + int(c-'0')
				haveDigit = true
			}
			j++
			continue
		}
		if c == ':' {
			// sub-parameter (kitty alternate keys): keep the primary value.
			skipSub = true
			haveDigit = true
			j++
			continue
		}
		if c == ';' {
			params = append(params, cur)
			cur = 0
			haveDigit = false
			skipSub = false
			j++
			continue
		}
		break
	}
	if haveDigit || len(params) > 0 {
		params = append(params, cur)
	}
	if j >= len(data) {
		return parsedEsc{kind: 0, key: key{kind: kEsc}}, 0
	}
	final := data[j]
	adv := j + 1

	mod := 0
	if len(params) >= 2 {
		mod = params[1]
	}
	shift, alt, ctrl := modBits(mod)

	// CSI u (kitty / fixterms): ESC [ code ; mod u
	if final == 'u' {
		if len(params) >= 1 {
			if k, ok := keyForCode(params[0], shift, alt, ctrl); ok {
				return parsedEsc{key: k}, adv
			}
		}
		return parsedEsc{kind: -1}, adv
	}

	// CSI ~ family: ESC [ n ; mod ~
	if final == '~' {
		n := 0
		if len(params) >= 1 {
			n = params[0]
		}
		// xterm modifyOtherKeys: ESC [ 27 ; mod ; code ~
		if n == 27 && len(params) >= 3 {
			s, al, c := modBits(params[1])
			if k, ok := keyForCode(params[2], s, al, c); ok {
				return parsedEsc{key: k}, adv
			}
			return parsedEsc{kind: -1}, adv
		}
		var k keyKind
		switch n {
		case 1, 7:
			k = kHome
		case 4, 8:
			k = kEnd
		case 3:
			k = kDelete
		case 5:
			k = kPgUp
		case 6:
			k = kPgDn
		case 2:
			return parsedEsc{kind: -1}, adv // Insert: ignore
		default:
			return parsedEsc{kind: -1}, adv
		}
		return parsedEsc{key: key{kind: k, shift: shift, alt: alt, ctrl: ctrl}}, adv
	}

	if k, ok := finalToArrow(final); ok {
		return parsedEsc{key: key{kind: k, shift: shift, alt: alt, ctrl: ctrl}}, adv
	}
	return parsedEsc{kind: -1}, adv
}

// keyForCode maps a CSI-u / modifyOtherKeys codepoint plus modifiers to a key.
// Uppercase letters normalize to lowercase + shift so bindings match either
// encoding a terminal chooses for e.g. ctrl+shift+f.
func keyForCode(code int, shift, alt, ctrl bool) (key, bool) {
	switch code {
	case 13:
		return key{kind: kEnter, shift: shift, alt: alt, ctrl: ctrl}, true
	case 9:
		if shift {
			return key{kind: kBacktab, alt: alt, ctrl: ctrl}, true
		}
		return key{kind: kTab, alt: alt, ctrl: ctrl}, true
	case 27:
		return key{kind: kEsc, shift: shift, alt: alt, ctrl: ctrl}, true
	case 8, 127:
		return key{kind: kBackspace, shift: shift, alt: alt, ctrl: ctrl}, true
	}
	if code < 0x20 || !utf8.ValidRune(rune(code)) {
		return key{}, false
	}
	r := rune(code)
	if r >= 'A' && r <= 'Z' {
		r = r - 'A' + 'a'
		shift = true
	}
	return key{kind: kRune, r: r, shift: shift, alt: alt, ctrl: ctrl}, true
}

func modBits(mod int) (shift, alt, ctrl bool) {
	if mod <= 0 {
		return false, false, false
	}
	m := mod - 1
	return m&1 != 0, m&2 != 0, m&4 != 0
}

func hasPrefix(b []byte, s string) bool {
	if len(b) < len(s) {
		return false
	}
	for i := 0; i < len(s); i++ {
		if b[i] != s[i] {
			return false
		}
	}
	return true
}
