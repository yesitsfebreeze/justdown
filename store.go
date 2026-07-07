package justdown

import (
	"database/sql"
	"fmt"
	"os"
	"strconv"
	"strings"

	_ "modernc.org/sqlite"
)

const StoreSchema int64 = 4

type Source int

const (
	SourceLocal Source = iota
	SourceOnline
)

func (s Source) IsLocal() bool { return s == SourceLocal }

func (s Source) Label() string {
	if s == SourceLocal {
		return "local"
	}
	return "online"
}

type Row struct {
	Source      Source
	Origin      string
	Key         string
	Name        string
	Kind        string
	Description string
	Purpose     string
	Tags        string
	Path        string
	UseWhen     string
	NotWhen     string
	Danger      string
	SideEffects string
	Requires    string
	Category    string
	Run         string
	HasFM       bool
	Links       []string
	Fuzzy       []string
}

func joinCSV(v []string) string { return strings.Join(v, ",") }

func RowFromNode(n *Node, source Source) Row {
	var links, fuzzy []string
	for _, t := range n.Links {
		form, term := ClassifyLink(t)
		if form == LinkFuzzy {
			fuzzy = append(fuzzy, term)
		} else {
			links = append(links, t)
		}
	}
	return Row{
		Source:      source,
		Key:         n.Key,
		Name:        n.Name,
		Kind:        n.Kind,
		Description: n.Description,
		Purpose:     n.Purpose,
		Tags:        joinCSV(n.Tags),
		Path:        n.Path,
		UseWhen:     joinCSV(n.UseWhen),
		NotWhen:     joinCSV(n.NotWhen),
		Danger:      n.Danger,
		SideEffects: joinCSV(n.SideEffects),
		Requires:    joinCSV(n.Requires),
		Category:    n.Category,
		Run:         n.Run,
		HasFM:       n.HasFrontmatter,
		Links:       links,
		Fuzzy:       fuzzy,
	}
}

func resolveEdges(idx *NameIndex, n *Node) (links, fuzzy []string) {
	for _, token := range n.Links {
		switch form, term := ClassifyLink(token); form {
		case LinkKey:
			links = append(links, term)
		case LinkName:
			if k, ok := idx.Resolve(term); ok {
				links = append(links, k)
			} else {
				links = append(links, term)
			}
		case LinkFuzzy:
			fuzzy = append(fuzzy, term)
		}
	}
	return links, fuzzy
}

func RowsFromNodes(nodes []Node, source Source) []Row {
	idx := BuildNameIndex(nodeIndexPairs(nodes))
	rows := make([]Row, len(nodes))
	for i := range nodes {
		row := RowFromNode(&nodes[i], source)
		row.Links, row.Fuzzy = resolveEdges(idx, &nodes[i])
		rows[i] = row
	}
	return rows
}

const schemaSQL = `
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE node (
  key          TEXT PRIMARY KEY,
  name         TEXT NOT NULL,
  kind         TEXT NOT NULL,
  description  TEXT NOT NULL,
  purpose      TEXT NOT NULL,
  tags         TEXT NOT NULL,
  path         TEXT NOT NULL,
  use_when     TEXT NOT NULL,
  not_when     TEXT NOT NULL,
  danger       TEXT NOT NULL,
  side_effects TEXT NOT NULL,
  requires     TEXT NOT NULL,
  category     TEXT NOT NULL,
  run          TEXT NOT NULL,
  has_fm       INTEGER NOT NULL
);
CREATE TABLE edge (
  src  TEXT NOT NULL,                      -- node key
  dst  TEXT NOT NULL,                      -- direct: target key (may be unresolved); fuzzy: the raw term
  kind TEXT NOT NULL DEFAULT 'direct'      -- 'direct' (@name/@dir/name) | 'fuzzy' (@?term)
);
CREATE INDEX idx_node_kind     ON node(kind);
CREATE INDEX idx_node_category ON node(category);
CREATE INDEX idx_node_name     ON node(name);
CREATE INDEX idx_edge_src      ON edge(src);
CREATE INDEX idx_edge_dst      ON edge(dst);
`

type Store struct {
	DB *sql.DB
}

func SchemaOf(path string) (int64, bool) {
	if _, err := os.Stat(path); err != nil {
		return 0, false
	}
	db, err := sql.Open("sqlite", path)
	if err != nil {
		return 0, false
	}
	defer db.Close()
	var v string
	if err := db.QueryRow("SELECT value FROM meta WHERE key = 'schema'").Scan(&v); err != nil {
		return 0, false
	}
	n, err := strconv.ParseInt(v, 10, 64)
	if err != nil {
		return 0, false
	}
	return n, true
}

func OpenStore(path string) (*Store, error) {
	db, err := sql.Open("sqlite", path)
	if err != nil {
		return nil, err
	}
	return &Store{DB: db}, nil
}

func (s *Store) Close() error { return s.DB.Close() }

func (s *Store) LoadRows(source Source) ([]Row, error) {
	links := map[string][]string{}
	fuzzy := map[string][]string{}
	hasKind := true
	if probe, err := s.DB.Query("SELECT src,dst,kind FROM edge LIMIT 0"); err != nil {
		hasKind = false
	} else {
		probe.Close()
	}
	esql := "SELECT src,dst,kind FROM edge ORDER BY rowid"
	if !hasKind {
		esql = "SELECT src,dst FROM edge ORDER BY rowid"
	}
	erows, err := s.DB.Query(esql)
	if err != nil {
		return nil, err
	}
	for erows.Next() {
		var src, dst, kind string
		if hasKind {
			if err := erows.Scan(&src, &dst, &kind); err != nil {
				erows.Close()
				return nil, err
			}
		} else {
			if err := erows.Scan(&src, &dst); err != nil {
				erows.Close()
				return nil, err
			}
		}
		if hasKind && kind == "fuzzy" {
			fuzzy[src] = append(fuzzy[src], dst)
		} else {
			links[src] = append(links[src], dst)
		}
	}
	if err := erows.Err(); err != nil {
		erows.Close()
		return nil, err
	}
	erows.Close()

	nrows, err := s.DB.Query(
		`SELECT key,name,kind,description,purpose,tags,path,use_when,not_when,
		        danger,side_effects,requires,category,run,has_fm
		 FROM node ORDER BY key`)
	if err != nil {
		return nil, err
	}
	defer nrows.Close()
	var out []Row
	for nrows.Next() {
		var r Row
		var hasFM int64
		if err := nrows.Scan(&r.Key, &r.Name, &r.Kind, &r.Description, &r.Purpose,
			&r.Tags, &r.Path, &r.UseWhen, &r.NotWhen, &r.Danger, &r.SideEffects,
			&r.Requires, &r.Category, &r.Run, &hasFM); err != nil {
			return nil, err
		}
		r.Source = source
		r.HasFM = hasFM != 0
		r.Links = links[r.Key]
		r.Fuzzy = fuzzy[r.Key]
		out = append(out, r)
	}
	return out, nrows.Err()
}

func BuildStore(path string, nodes []Node, producer string) error {
	_ = os.Remove(path)
	db, err := sql.Open("sqlite", path)
	if err != nil {
		return err
	}
	defer db.Close()
	if _, err := db.Exec(schemaSQL); err != nil {
		return err
	}

	idx := BuildNameIndex(nodeIndexPairs(nodes))

	tx, err := db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	insNode, err := tx.Prepare(
		`INSERT OR REPLACE INTO node
		 (key,name,kind,description,purpose,tags,path,use_when,not_when,
		  danger,side_effects,requires,category,run,has_fm)
		 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`)
	if err != nil {
		return err
	}
	insEdge, err := tx.Prepare("INSERT INTO edge (src,dst,kind) VALUES (?,?,?)")
	if err != nil {
		return err
	}
	for i := range nodes {
		n := &nodes[i]
		hasFM := 0
		if n.HasFrontmatter {
			hasFM = 1
		}
		if _, err := insNode.Exec(n.Key, n.Name, n.Kind, n.Description, n.Purpose,
			joinCSV(n.Tags), n.Path, joinCSV(n.UseWhen), joinCSV(n.NotWhen),
			n.Danger, joinCSV(n.SideEffects), joinCSV(n.Requires),
			n.Category, n.Run, hasFM); err != nil {
			return err
		}
		direct, fuzz := resolveEdges(idx, n)
		for _, dst := range direct {
			if _, err := insEdge.Exec(n.Key, dst, "direct"); err != nil {
				return err
			}
		}
		for _, term := range fuzz {
			if _, err := insEdge.Exec(n.Key, term, "fuzzy"); err != nil {
				return err
			}
		}
	}
	insMeta, err := tx.Prepare("INSERT OR REPLACE INTO meta (key,value) VALUES (?,?)")
	if err != nil {
		return err
	}
	for _, kv := range [][2]string{
		{"schema", strconv.FormatInt(StoreSchema, 10)},
		{"producer", producer},
		{"node_count", fmt.Sprintf("%d", len(nodes))},
	} {
		if _, err := insMeta.Exec(kv[0], kv[1]); err != nil {
			return err
		}
	}
	return tx.Commit()
}
