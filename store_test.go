package justdown

import (
	"path/filepath"
	"reflect"
	"testing"
)

func TestBuildResolvesDirectNamesAndMarksFuzzyEdges(t *testing.T) {
	store := filepath.Join(t.TempDir(), "graph.db")

	target := Parse("library/ui/glassmorphism.jd",
		"---\nname: glass\nkind: knowledge\ndescription: d\n---\nbody\n")
	src := Parse("library/ui/card.jd",
		"---\nname: card\nkind: knowledge\ndescription: d\n---\nSee @glassmorphism and @?soft and @ghost.\n")
	if err := BuildStore(store, []Node{src, target}, "test"); err != nil {
		t.Fatal(err)
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
	var card *Row
	for i := range rows {
		if rows[i].Key == "ui/card" {
			card = &rows[i]
		}
	}
	if card == nil {
		t.Fatal("ui/card missing")
	}
	if !reflect.DeepEqual(card.Links, []string{"ui/glassmorphism", "ghost"}) {
		t.Fatalf("links: %v", card.Links)
	}
	if !reflect.DeepEqual(card.Fuzzy, []string{"soft"}) {
		t.Fatalf("fuzzy: %v", card.Fuzzy)
	}
}

func TestSchemaOf(t *testing.T) {
	store := filepath.Join(t.TempDir(), "graph.db")
	if _, ok := SchemaOf(store); ok {
		t.Fatal("absent file has no schema")
	}
	if err := BuildStore(store, nil, "test"); err != nil {
		t.Fatal(err)
	}
	v, ok := SchemaOf(store)
	if !ok || v != StoreSchema {
		t.Fatalf("schema: %d %v", v, ok)
	}
}
