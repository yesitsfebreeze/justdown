package justdown

import "strings"

const FuzzyPrefix = "?"

type LinkForm int

const (
	LinkKey LinkForm = iota
	LinkName
	LinkFuzzy
)

func ClassifyLink(token string) (LinkForm, string) {
	if t, ok := strings.CutPrefix(token, FuzzyPrefix); ok {
		return LinkFuzzy, t
	}
	if strings.Contains(token, "/") {
		return LinkKey, token
	}
	return LinkName, token
}

func Leaf(key string) string {
	if i := strings.LastIndexByte(key, '/'); i >= 0 {
		return key[i+1:]
	}
	return key
}

type NameIndex struct {
	m map[string][]string
}

func BuildNameIndex(pairs [][2]string) *NameIndex {
	m := map[string][]string{}
	add := func(ident, key string) {
		for _, k := range m[ident] {
			if k == key {
				return
			}
		}
		m[ident] = append(m[ident], key)
	}
	for _, p := range pairs {
		key, name := p[0], p[1]
		add(key, key)
		if name != "" {
			add(name, key)
		}
		add(Leaf(key), key)
	}
	return &NameIndex{m: m}
}

func nodeIndexPairs(nodes []Node) [][2]string {
	pairs := make([][2]string, len(nodes))
	for i, n := range nodes {
		pairs[i] = [2]string{n.Key, n.Name}
	}
	return pairs
}

func rowIndexPairs(rows []Row) [][2]string {
	pairs := make([][2]string, len(rows))
	for i := range rows {
		pairs[i] = [2]string{rows[i].Key, rows[i].Name}
	}
	return pairs
}

func (idx *NameIndex) Resolve(token string) (string, bool) {
	if v := idx.m[token]; len(v) == 1 {
		return v[0], true
	}
	return "", false
}

func (idx *NameIndex) Candidates(token string) []string {
	return idx.m[token]
}

func ResolveTerm(rows []Row, term string, fuzzy bool, limit int) ([]*Row, string) {
	if fuzzy {
		scored := Rank(rows, term, "", "")
		if len(scored) > limit {
			scored = scored[:limit]
		}
		matches := make([]*Row, len(scored))
		for i, s := range scored {
			matches[i] = s.Row
		}
		return matches, ""
	}
	idx := BuildNameIndex(rowIndexPairs(rows))
	resolved, _ := idx.Resolve(term)
	matches := ResolvePrefix(rows, term)
	if len(matches) > limit {
		matches = matches[:limit]
	}
	return matches, resolved
}
