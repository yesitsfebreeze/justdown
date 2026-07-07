package tests

import (
	justdown "github.com/yesitsfebreeze/justdown/src"

	"fmt"
	"strings"
	"testing"
)

func block(inner string) string {
	return fmt.Sprintf("---\nkind: tool\n---\n\n```just\n%s\n```\n", inner)
}

func msgs(findings []justdown.Finding) []string {
	out := make([]string, len(findings))
	for i, f := range findings {
		out[i] = f.Message
	}
	return out
}

func anyContains(list []string, sub string) bool {
	for _, m := range list {
		if strings.Contains(m, sub) {
			return true
		}
	}
	return false
}

func TestFlagsOverlappingVariants(t *testing.T) {
	errs := justdown.PlatformErrors(block("[unix, wsl]\nr:\n  a\n[wsl]\nr:\n  b"))
	if len(errs) != 1 {
		t.Fatalf("%v", errs)
	}
	if !strings.Contains(errs[0], "recipe `r`") || !strings.Contains(errs[0], "[wsl]") {
		t.Fatalf("%v", errs)
	}
}

func TestAcceptsMutuallyExclusiveVariants(t *testing.T) {
	errs := justdown.PlatformErrors(block("[unix, wsl]\nr:\n  a\n[macos]\nr:\n  b\n[windows]\nr:\n  c"))
	if len(errs) != 0 {
		t.Fatalf("%v", errs)
	}
}

func TestIgnoresFilesWithoutTheConvention(t *testing.T) {
	if errs := justdown.PlatformErrors(block("a:\n  one\nb:\n  two")); len(errs) != 0 {
		t.Fatalf("%v", errs)
	}
}

func TestRecipeNameDetectsHeadersOnly(t *testing.T) {
	if n, ok := justdown.RecipeName("open target:"); !ok || n != "open" {
		t.Fatalf("%q %v", n, ok)
	}
	if n, ok := justdown.RecipeName("check host count=\"5\":"); !ok || n != "check" {
		t.Fatalf("%q %v", n, ok)
	}
	for _, bad := range []string{"  xdg-open x", "# comment", "[unix]", "x := 1"} {
		if _, ok := justdown.RecipeName(bad); ok {
			t.Fatalf("%q must not be a header", bad)
		}
	}
}

func TestLintNodeFlagsMissingRequiredFields(t *testing.T) {
	n := justdown.Parse("library/x/foo.jd", "---\nkind: tool\n---\nbody\n")
	m := msgs(justdown.LintNode(&n, "body\n"))
	for _, want := range []string{
		"missing required field: name",
		"missing required field: description",
		"tool has no `run:` recipe",
	} {
		if !anyContains(m, want) {
			t.Fatalf("missing %q in %v", want, m)
		}
	}
}

func TestLintNodeFlagsBadEnums(t *testing.T) {
	n := justdown.Parse("library/x/foo.jd",
		"---\nname: foo\ndescription: d\nkind: gadget\ndanger: spicy\nrun: go\n---\nbody\n")
	m := msgs(justdown.LintNode(&n, "body\n"))
	if !anyContains(m, "invalid kind: gadget") || !anyContains(m, "invalid danger: spicy") {
		t.Fatalf("%v", m)
	}
}

func TestLintNodeNoFrontmatterIsOnlyFinding(t *testing.T) {
	n := justdown.Parse("library/x/foo.jd", "no frontmatter here\n")
	f := justdown.LintNode(&n, "no frontmatter here\n")
	if len(f) != 1 || !f[0].IsError() || !strings.Contains(f[0].Message, "no frontmatter") {
		t.Fatalf("%+v", f)
	}
}

func TestLintNodeCleanToolHasNoErrors(t *testing.T) {
	n := justdown.Parse("library/x/foo.jd",
		"---\nname: foo\ndescription: does a thing\nkind: tool\nrun: go\nuse_when: [do a thing]\n---\nbody\n")
	for _, f := range justdown.LintNode(&n, "body\n") {
		if f.IsError() {
			t.Fatalf("unexpected error: %+v", f)
		}
	}
}
