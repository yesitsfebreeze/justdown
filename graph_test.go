package justdown

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func write(t *testing.T, path, body string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(body), 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestLaterRootOverridesByKey(t *testing.T) {
	dir := t.TempDir()
	write(t, filepath.Join(dir, "base/cat/tool.jd"), "---\nname: from_base\nkind: tool\n---\nbody\n")
	write(t, filepath.Join(dir, "over/cat/tool.jd"), "---\nname: from_over\nkind: tool\n---\nbody\n")

	store := filepath.Join(dir, "graph.db")
	n, err := BuildIndex(store, []Root{
		NewRoot(filepath.Join(dir, "base")),
		NewRoot(filepath.Join(dir, "over")),
	}, "test")
	if err != nil {
		t.Fatal(err)
	}
	if n != 1 {
		t.Fatalf("same key collapses to one node, got %d", n)
	}
	s, err := OpenStore(store)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	rows, err := s.LoadRows(SourceLocal)
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 1 || rows[0].Name != "from_over" {
		t.Fatalf("later root wins: %+v", rows)
	}
}

func TestFindHomesDiscoversNestedPrunesHeavyAndOrdersDeeperFirst(t *testing.T) {
	root := t.TempDir()
	for _, d := range []string{
		".jd/library",
		"packages/x/.jd/library",
		"node_modules/dep/.jd/library",
		".jd/library/.jd",
	} {
		if err := os.MkdirAll(filepath.Join(root, d), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	homes := FindJDHomes(root)
	has := func(p string) bool {
		for _, h := range homes {
			if h == p {
				return true
			}
		}
		return false
	}
	if !has(filepath.Join(root, ".jd")) {
		t.Fatalf("root home found: %v", homes)
	}
	if !has(filepath.Join(root, "packages/x/.jd")) {
		t.Fatalf("nested home found: %v", homes)
	}
	for _, h := range homes {
		if strings.HasPrefix(h, filepath.Join(root, "node_modules")) {
			t.Fatalf("node_modules pruned: %v", homes)
		}
		if strings.HasSuffix(h, filepath.Join("library", ".jd")) {
			t.Fatalf("a home's own subtree is not descended: %v", homes)
		}
	}
	deep, shallow := -1, -1
	for i, h := range homes {
		if h == filepath.Join(root, "packages/x/.jd") {
			deep = i
		}
		if h == filepath.Join(root, ".jd") {
			shallow = i
		}
	}
	if deep >= shallow {
		t.Fatalf("deeper home sorts first: %v", homes)
	}
}
