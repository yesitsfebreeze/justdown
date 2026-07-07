package justdown

import "testing"

func srow(key, name, kind, useWhen, notWhen, purpose string) Row {
	return Row{
		Source:      SourceLocal,
		Key:         key,
		Name:        name,
		Kind:        kind,
		Description: purpose,
		Purpose:     purpose,
		UseWhen:     useWhen,
		NotWhen:     notWhen,
		HasFM:       true,
	}
}

func TestResolvePrefixRanksExactThenPrefixThenSubstring(t *testing.T) {
	rows := []Row{
		srow("soft-ui/glass", "glass", "tool", "", "", ""),
		srow("ui/glassmorphism", "glassmorphism", "tool", "", "", ""),
		srow("x/subglass", "subglass", "tool", "", "", ""),
	}
	out := ResolvePrefix(rows, "glass")
	keys := make([]string, len(out))
	for i, r := range out {
		keys[i] = r.Key
	}
	want := []string{"soft-ui/glass", "ui/glassmorphism", "x/subglass"}
	for i := range want {
		if keys[i] != want[i] {
			t.Fatalf("order: %v", keys)
		}
	}
}

func TestNameOutscoresPurposeAndNotWhenVetoes(t *testing.T) {
	rows := []Row{
		srow("search/rg", "search_rg", "tool", "grep the repo", "", "ripgrep search"),
		srow("web/fetch", "web_fetch", "tool", "download a url", "do not grep", "fetch a web page"),
	}
	out := Rank(rows, "grep search", "tool", "")
	if len(out) != 1 {
		t.Fatalf("not_when veto drops web_fetch: %d", len(out))
	}
	if out[0].Row.Name != "search_rg" || out[0].Score < 3 {
		t.Fatalf("%+v", out[0])
	}
}

func TestFuzzyNameSubsequenceMatches(t *testing.T) {
	terms := SearchTerms("relse")
	hit, snippet := MatchNameContent("meta/tools/release.jd", "", terms)
	if !hit {
		t.Fatal("subsequence 'relse' must hit 'release'")
	}
	if snippet != "" {
		t.Fatal("a name-only hit carries no snippet")
	}
	if miss, _ := MatchNameContent("meta/tools/gate.jd", "", terms); miss {
		t.Fatal("gate must not match")
	}
}

func TestContentSubstringMatchesAndSnippets(t *testing.T) {
	raw := "---\nname: rg\n---\n\nUse Vim keys to navigate results.\n"
	hit, snippet := MatchNameContent("search/rg.jd", raw, SearchTerms("VIM"))
	if !hit {
		t.Fatal("content match is case-insensitive")
	}
	if snippet != "Use Vim keys to navigate results." {
		t.Fatalf("snippet: %q", snippet)
	}
}

func TestEveryTermMustMatchNameOrContent(t *testing.T) {
	raw := "body mentions vim here"
	if hit, _ := MatchNameContent("search/rg.jd", raw, SearchTerms("vim rg")); !hit {
		t.Fatal("'rg' hits name, 'vim' hits content")
	}
	miss, snippet := MatchNameContent("search/rg.jd", raw, SearchTerms("vim zzz"))
	if miss || snippet != "" {
		t.Fatal("'zzz' hits nothing")
	}
}

func TestKindFilterExcludesNonTools(t *testing.T) {
	rows := []Row{
		srow("x/tool", "a_tool", "tool", "do x", "", "does x"),
		srow("x/note", "a_note", "knowledge", "do x", "", "about x"),
	}
	out := Rank(rows, "x", "tool", "")
	if len(out) != 1 || out[0].Row.Kind != "tool" {
		t.Fatalf("%+v", out)
	}
}
