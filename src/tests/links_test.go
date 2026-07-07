package tests

import (
	justdown "github.com/yesitsfebreeze/justdown/src"

	"reflect"
	"testing"
)

func TestClassifyDistinguishesTheThreeForms(t *testing.T) {
	if f, v := justdown.ClassifyLink("dir/name"); f != justdown.LinkKey || v != "dir/name" {
		t.Fatalf("key: %v %q", f, v)
	}
	if f, v := justdown.ClassifyLink("glassmorphism"); f != justdown.LinkName || v != "glassmorphism" {
		t.Fatalf("name: %v %q", f, v)
	}
	if f, v := justdown.ClassifyLink("?soft"); f != justdown.LinkFuzzy || v != "soft" {
		t.Fatalf("fuzzy: %v %q", f, v)
	}
}

func TestLeafIsLastSegment(t *testing.T) {
	if justdown.Leaf("soft-ui/glassmorphism") != "glassmorphism" || justdown.Leaf("glassmorphism") != "glassmorphism" {
		t.Fatal("leaf")
	}
}

func TestNameResolvesViaKeyNameOrLeaf(t *testing.T) {
	idx := justdown.BuildNameIndex([][2]string{
		{"soft-ui/glassmorphism", "glass"},
		{"vcs/gh/release", "gh_release"},
	})
	for _, tc := range [][2]string{
		{"glassmorphism", "soft-ui/glassmorphism"}, // by leaf
		{"glass", "soft-ui/glassmorphism"},         // by frontmatter name
		{"vcs/gh/release", "vcs/gh/release"},       // by full key
	} {
		k, ok := idx.Resolve(tc[0])
		if !ok || k != tc[1] {
			t.Fatalf("resolve %q: %q %v", tc[0], k, ok)
		}
	}
	if _, ok := idx.Resolve("nope"); ok {
		t.Fatal("nope must not resolve")
	}
}

func TestCollideOnLeafIsAmbiguous(t *testing.T) {
	idx := justdown.BuildNameIndex([][2]string{
		{"meta/release", "a"},
		{"gh/release", "b"},
	})
	if _, ok := idx.Resolve("release"); ok {
		t.Fatal("ambiguous leaf must not resolve")
	}
	keys := idx.Candidates("release")
	if len(keys) != 2 {
		t.Fatalf("candidates: %v", keys)
	}
	want := map[string]bool{"meta/release": true, "gh/release": true}
	for _, k := range keys {
		if !want[k] {
			t.Fatalf("unexpected candidate %q in %v", k, keys)
		}
	}
}

func TestResolveTermDirectAndFuzzy(t *testing.T) {
	rows := []justdown.Row{
		{Source: justdown.SourceLocal, Key: "soft-ui/glass", Name: "glass", Kind: "tool"},
		{Source: justdown.SourceLocal, Key: "ui/glassmorphism", Name: "glassmorphism", Kind: "tool",
			UseWhen: "soft glass ui"},
	}
	matches, resolved := justdown.ResolveTerm(rows, "glass", false, 10)
	if resolved != "soft-ui/glass" {
		t.Fatalf("resolved: %q", resolved)
	}
	keys := make([]string, len(matches))
	for i, m := range matches {
		keys[i] = m.Key
	}
	if !reflect.DeepEqual(keys, []string{"soft-ui/glass", "ui/glassmorphism"}) {
		t.Fatalf("direct matches: %v", keys)
	}
	fmatches, fresolved := justdown.ResolveTerm(rows, "soft", true, 10)
	if fresolved != "" {
		t.Fatalf("fuzzy never resolves a canonical key: %q", fresolved)
	}
	if len(fmatches) != 1 || fmatches[0].Key != "ui/glassmorphism" {
		t.Fatalf("fuzzy matches: %+v", fmatches)
	}
}
