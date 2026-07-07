package tests

import (
	justdown "github.com/yesitsfebreeze/justdown/src"

	"path/filepath"
	"reflect"
	"testing"
)

func TestBuildResolvesDirectNamesAndMarksFuzzyEdges(t *testing.T) {
	store := filepath.Join(t.TempDir(), "graph.db")

	target := justdown.Parse("library/ui/glassmorphism.jd",
		"---\nname: glass\nkind: knowledge\ndescription: d\n---\nbody\n")
	src := justdown.Parse("library/ui/card.jd",
		"---\nname: card\nkind: knowledge\ndescription: d\n---\nSee @glassmorphism and @?soft and @ghost.\n")
	if err := justdown.BuildStore(store, []justdown.Node{src, target}, "test"); err != nil {
		t.Fatal(err)
	}

	s, err := justdown.OpenStore(store)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	rows, err := s.LoadRows(justdown.SourceLocal)
	if err != nil {
		t.Fatal(err)
	}
	var card *justdown.Row
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
	if _, ok := justdown.SchemaOf(store); ok {
		t.Fatal("absent file has no schema")
	}
	if err := justdown.BuildStore(store, nil, "test"); err != nil {
		t.Fatal(err)
	}
	v, ok := justdown.SchemaOf(store)
	if !ok || v != justdown.StoreSchema {
		t.Fatalf("schema: %d %v", v, ok)
	}
}
