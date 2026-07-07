package justdown

import "strings"

// Lines matches Rust's str::lines: \r\n handled, no empty tail after a
// trailing newline. Output parity with the original CLI depends on this shape.
func Lines(s string) []string {
	if s == "" {
		return nil
	}
	s = strings.TrimSuffix(s, "\n")
	parts := strings.Split(s, "\n")
	for i, p := range parts {
		parts[i] = strings.TrimSuffix(p, "\r")
	}
	return parts
}
