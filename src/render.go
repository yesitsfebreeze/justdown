package justdown

import "strings"

// Render is single-pass and non-recursive: injected values are never
// re-scanned, so untrusted host state cannot smuggle in further <<escapes>>.
// Unknown names and malformed escapes pass through verbatim; `<<<<` emits a
// literal `<<`.

type Vars map[string]string

func isValidName(name string) bool {
	if name == "" {
		return false
	}
	for i := 0; i < len(name); i++ {
		b := name[i]
		if !(b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z' || b >= '0' && b <= '9' || b == '_') {
			return false
		}
	}
	return true
}

func Render(template string, vars Vars) string {
	var out strings.Builder
	out.Grow(len(template))
	rest := template
	for {
		pos := strings.Index(rest, "<<")
		if pos < 0 {
			break
		}
		out.WriteString(rest[:pos])
		after := rest[pos+2:]
		if tail, ok := strings.CutPrefix(after, "<<"); ok {
			out.WriteString("<<")
			rest = tail
			continue
		}
		if close := strings.Index(after, ">>"); close >= 0 {
			name := after[:close]
			if isValidName(name) {
				if val, ok := vars[name]; ok {
					out.WriteString(val)
				} else {
					out.WriteString("<<")
					out.WriteString(name)
					out.WriteString(">>")
				}
				rest = after[close+2:]
				continue
			}
		}
		out.WriteString("<<")
		rest = after
	}
	out.WriteString(rest)
	return out.String()
}
