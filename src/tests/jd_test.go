package tests

import (
	justdown "github.com/yesitsfebreeze/justdown/src"

	"reflect"
	"testing"
)

func TestKeyFromThreeLevelPath(t *testing.T) {
	k, c := justdown.KeyAndCategory("library/security/crypto/gpg.jd")
	if k != "crypto/gpg" || c != "crypto" {
		t.Fatalf("got %q %q", k, c)
	}
}

func TestParsesFrontmatterAndLinks(t *testing.T) {
	src := "---\nname: tools_release\nkind: tool\ndescription: Cut a release.\ntags: [release, publish, ci]\nrun: release\n---\n\nUses @tools/gate and @tools/gate again, plus @cert/openssl.\n"
	n := justdown.Parse("library/meta/tools/release.jd", src)
	if n.Name != "tools_release" || n.Kind != "tool" || n.Key != "tools/release" ||
		n.Category != "tools" || n.Run != "release" {
		t.Fatalf("fields: %+v", n)
	}
	if !reflect.DeepEqual(n.Tags, []string{"release", "publish", "ci"}) {
		t.Fatalf("tags: %v", n.Tags)
	}
	if !reflect.DeepEqual(n.Links, []string{"tools/gate", "cert/openssl"}) {
		t.Fatalf("links: %v", n.Links)
	}
	if !n.HasFrontmatter {
		t.Fatal("has_frontmatter")
	}
}

func TestScansDirectNameAndFuzzyLinks(t *testing.T) {
	src := "---\nname: t\nkind: knowledge\ndescription: d\n---\nSee @glassmorphism and @?soft, plus @soft-ui/glass and @glassmorphism again.\n"
	n := justdown.Parse("library/x/t.jd", src)
	if !reflect.DeepEqual(n.Links, []string{"glassmorphism", "?soft", "soft-ui/glass"}) {
		t.Fatalf("links: %v", n.Links)
	}
}

func TestBareAtAndTrailingSectionDoNotCapture(t *testing.T) {
	n := justdown.Parse("library/x/t.jd",
		"---\nname: t\nkind: knowledge\ndescription: d\n---\nemail a@ b, and @ alone, and @glass#tips here.\n")
	if !reflect.DeepEqual(n.Links, []string{"glass"}) {
		t.Fatalf("links: %v", n.Links)
	}
}

func TestSkipsLinksInInlineAndFencedCode(t *testing.T) {
	src := "---\nname: t\nkind: knowledge\ndescription: d\n---\nProse @glass and `@daily` cron.\n\n```just\nr:\n  @echo hi\n  npm i x@latest\n```\n\nMore @a/b here.\n"
	n := justdown.Parse("library/x/t.jd", src)
	if !reflect.DeepEqual(n.Links, []string{"glass", "a/b"}) {
		t.Fatalf("links: %v", n.Links)
	}
}

func TestNameFallsBackToKey(t *testing.T) {
	n := justdown.Parse("library/x/foo.jd", "---\nkind: tool\n---\nbody\n")
	if n.Name != "x/foo" || n.Purpose != "x/foo" || n.NameGiven {
		t.Fatalf("%+v", n)
	}
}

func TestParsesBlockStyleArrays(t *testing.T) {
	src := "---\nname: t\nkind: tool\ntags:\n  - alpha\n  - beta\nuse_when:\n  - go to definition\n  - jump to symbol\n---\nbody\n"
	n := justdown.Parse("library/x/t.jd", src)
	if !reflect.DeepEqual(n.Tags, []string{"alpha", "beta"}) {
		t.Fatalf("tags: %v", n.Tags)
	}
	if !reflect.DeepEqual(n.UseWhen, []string{"go to definition", "jump to symbol"}) {
		t.Fatalf("use_when: %v", n.UseWhen)
	}
}

func TestQuotedItemWithFlowCharIsPreserved(t *testing.T) {
	src := "---\nname: t\nkind: tool\nuse_when: [tag stack, \"ctrl-]\", more]\n---\nbody\n"
	n := justdown.Parse("library/x/t.jd", src)
	if !reflect.DeepEqual(n.UseWhen, []string{"tag stack", "ctrl-]", "more"}) {
		t.Fatalf("use_when: %v", n.UseWhen)
	}
}

func TestMalformedFrontmatterDegradesWithoutPanicking(t *testing.T) {
	src := "---\nname: t\ndescription: a tool: that breaks yaml\n---\nbody @a/b\n"
	n := justdown.Parse("library/x/t.jd", src)
	if !n.HasFrontmatter {
		t.Fatal("frontmatter block present")
	}
	if n.NameGiven {
		t.Fatal("name must not be given after degrade")
	}
	if n.Name != "x/t" {
		t.Fatalf("name fallback: %q", n.Name)
	}
	if !reflect.DeepEqual(n.Links, []string{"a/b"}) {
		t.Fatalf("body still scanned: %v", n.Links)
	}
}
