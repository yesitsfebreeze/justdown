package justdown

import (
	"fmt"
	"sort"
	"strings"
)

var Kinds = []string{"tool", "agent", "knowledge", "workflow"}

var Dangers = []string{"none", "low", "medium", "high"}

type Severity int

const (
	SeverityError Severity = iota
	SeverityWarn
)

type Finding struct {
	Severity Severity
	Message  string
}

func LintError(message string) Finding { return Finding{SeverityError, message} }

func LintWarn(message string) Finding { return Finding{SeverityWarn, message} }

func (f Finding) IsError() bool { return f.Severity == SeverityError }

func RecipeName(line string) (string, bool) {
	if line == "" || strings.ContainsAny(line[:1], " \t#[") || strings.Contains(line, ":=") {
		return "", false
	}
	colon := strings.IndexByte(line, ':')
	if colon < 0 {
		return "", false
	}
	fields := strings.Fields(line[:colon])
	if len(fields) == 0 {
		return "", false
	}
	nm := fields[0]
	for i := 0; i < len(nm); i++ {
		c := nm[i]
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_' || c == '-') {
			return "", false
		}
	}
	return nm, true
}

func PlatformErrors(body string) []string {
	lines := RawToolsLines(body)
	uses := false
	for _, l := range lines {
		if ParsePlatformAttr(l) != nil {
			uses = true
			break
		}
	}
	if !uses {
		return nil
	}
	var errs []string
	for _, plat := range Platforms {
		resolved := Platsel(lines, plat)
		counts := map[string]int{}
		for _, l := range resolved {
			if n, ok := RecipeName(l); ok {
				counts[n]++
			}
		}
		var dups []string
		for n, c := range counts {
			if c > 1 {
				dups = append(dups, n)
			}
		}
		sort.Strings(dups)
		for _, n := range dups {
			errs = append(errs, fmt.Sprintf(
				"recipe `%s` has overlapping platform variants on [%s] (would serve a duplicate definition)", n, plat))
		}
	}
	return errs
}

func LintNode(node *Node, body string) []Finding {
	var out []Finding
	if !node.HasFrontmatter {
		return append(out, LintError("no frontmatter block"))
	}
	if !node.NameGiven {
		out = append(out, LintError("missing required field: name"))
	}
	if node.Description == "" {
		out = append(out, LintError("missing required field: description"))
	}
	if node.Kind == "" {
		out = append(out, LintError("missing required field: kind"))
	} else if !contains(Kinds, node.Kind) {
		out = append(out, LintError(fmt.Sprintf(
			"invalid kind: %s (want tool|agent|knowledge|workflow)", node.Kind)))
	}
	if node.Kind == "tool" && node.Run == "" {
		out = append(out, LintError("tool has no `run:` recipe"))
	}
	if node.Danger != "" && !contains(Dangers, node.Danger) {
		out = append(out, LintError(fmt.Sprintf(
			"invalid danger: %s (want none|low|medium|high)", node.Danger)))
	}
	if (node.Kind == "tool" || node.Kind == "workflow") && len(node.UseWhen) == 0 {
		out = append(out, LintWarn("no use_when (retrieval leans on description alone)"))
	}
	for _, m := range PlatformErrors(body) {
		out = append(out, LintError(m))
	}
	return out
}

func contains(list []string, s string) bool {
	for _, v := range list {
		if v == s {
			return true
		}
	}
	return false
}
