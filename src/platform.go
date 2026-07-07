package justdown

import (
	"os"
	"runtime"
	"strings"
)

var Platforms = []string{"unix", "macos", "windows", "wsl"}

func HostPlatform() string {
	if p := os.Getenv("JD_PLATFORM"); p != "" {
		return p
	}
	switch runtime.GOOS {
	case "darwin":
		return "macos"
	case "windows":
		return "windows"
	case "linux":
		wsl := os.Getenv("WSL_DISTRO_NAME") != ""
		if !wsl {
			if b, err := os.ReadFile("/proc/version"); err == nil {
				wsl = strings.Contains(strings.ToLower(string(b)), "microsoft")
			}
		}
		if wsl {
			return "wsl"
		}
		return "unix"
	default:
		return "unix"
	}
}

func ParsePlatformAttr(line string) []string {
	s := strings.TrimSpace(line)
	inner, ok := strings.CutPrefix(s, "[")
	if !ok {
		return nil
	}
	inner, ok = strings.CutSuffix(inner, "]")
	if !ok {
		return nil
	}
	var tags []string
	for _, part := range strings.Split(inner, ",") {
		switch t := strings.TrimSpace(part); t {
		case "unix", "macos", "darwin", "windows", "wsl":
			tags = append(tags, t)
		default:
			return nil
		}
	}
	return tags
}

func RawToolsLines(body string) []string {
	var out []string
	inJust := false
	for _, line := range Lines(body) {
		if strings.HasPrefix(line, "```just") {
			inJust = true
			continue
		}
		if inJust && strings.HasPrefix(line, "```") {
			inJust = false
			continue
		}
		if inJust {
			out = append(out, line)
		}
	}
	return out
}

func Platsel(lines []string, plat string) []string {
	out := []string{}
	pend := false    // previous line was a platform attr; next is the header
	guarded := false // inside a guarded recipe's body
	keep := true     // emit the current guarded block?
	for _, line := range lines {
		if tags := ParsePlatformAttr(line); tags != nil {
			keep = false
			for _, t := range tags {
				if t == "darwin" {
					t = "macos"
				}
				if t == plat {
					keep = true
					break
				}
			}
			pend = true
			guarded = false
			continue
		}
		if pend {
			pend = false
			guarded = true
			if keep {
				out = append(out, line)
			}
			continue
		}
		if guarded {
			if line == "" || strings.HasPrefix(line, " ") || strings.HasPrefix(line, "\t") {
				if keep {
					out = append(out, line)
				}
				continue
			}
			guarded = false
			keep = true
		}
		out = append(out, line)
	}
	return out
}

func SelectForHost(justfile string) string {
	return strings.Join(Platsel(Lines(justfile), HostPlatform()), "\n")
}
