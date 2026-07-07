package justdown

import (
	"sort"
	"strings"
)

var Stopwords = []string{
	"a", "an", "and", "or", "the", "of", "to", "in", "on", "at", "is", "it", "its", "be", "as",
	"do", "for", "my", "our", "your", "this", "that", "with", "from", "by",
}

func isStopword(w string) bool {
	for _, s := range Stopwords {
		if w == s {
			return true
		}
	}
	return false
}

func Words(s string) []string {
	var out []string
	start := -1
	for i := 0; i < len(s); i++ {
		c := s[i]
		ok := (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '+'
		if ok {
			if start < 0 {
				start = i
			}
		} else if start >= 0 {
			out = append(out, s[start:i])
			start = -1
		}
	}
	if start >= 0 {
		out = append(out, s[start:])
	}
	return out
}

func fhit(field, term string) bool {
	for _, w := range Words(field) {
		if strings.Contains(w, term) {
			return true
		}
	}
	return false
}

func DegreeMap(rows []Row) map[string]int {
	indeg := map[string]int{}
	for i := range rows {
		for _, l := range rows[i].Links {
			indeg[l]++
		}
	}
	deg := map[string]int{}
	for i := range rows {
		deg[rows[i].Key] = len(rows[i].Links) + indeg[rows[i].Key]
	}
	return deg
}

type Scored struct {
	Score int64
	Row   *Row
}

func SearchTerms(query string) []string {
	return strings.Fields(strings.ToLower(query))
}

func Subsequence(hay []rune, needle string) bool {
	want := []rune(needle)
	wi := 0
	for _, c := range hay {
		if wi >= len(want) {
			return true
		}
		if c == want[wi] {
			wi++
		}
	}
	return wi >= len(want)
}

func MatchNameContent(label, raw string, terms []string) (bool, string) {
	hay := []rune(strings.ToLower(label))
	content := strings.ToLower(raw)
	snippet := ""
	for _, t := range terms {
		if Subsequence(hay, t) {
			continue
		}
		if strings.Contains(content, t) {
			if snippet == "" {
				for _, l := range Lines(raw) {
					if strings.Contains(strings.ToLower(l), t) {
						r := []rune(strings.TrimSpace(l))
						if len(r) > 120 {
							r = r[:120]
						}
						snippet = string(r)
						break
					}
				}
			}
			continue
		}
		return false, ""
	}
	return true, snippet
}

func ResolvePrefix(rows []Row, prefix string) []*Row {
	p := strings.ToLower(prefix)
	deg := DegreeMap(rows)

	quality := func(row *Row) (uint8, bool) {
		if p == "" {
			return 3, true
		}
		fields := []string{
			strings.ToLower(row.Key),
			strings.ToLower(row.Name),
			strings.ToLower(Leaf(row.Key)),
		}
		var best uint8
		found := false
		for _, f := range fields {
			var q uint8
			switch {
			case f == p:
				q = 0
			case strings.HasPrefix(f, p):
				q = 1
			case strings.Contains(f, p):
				q = 2
			default:
				continue
			}
			if !found || q < best {
				best = q
				found = true
			}
		}
		return best, found
	}

	type hit struct {
		q   uint8
		row *Row
	}
	var hits []hit
	for i := range rows {
		if q, ok := quality(&rows[i]); ok {
			hits = append(hits, hit{q, &rows[i]})
		}
	}
	sort.SliceStable(hits, func(a, b int) bool {
		if hits[a].q != hits[b].q {
			return hits[a].q < hits[b].q
		}
		da, db := deg[hits[a].row.Key], deg[hits[b].row.Key]
		if da != db {
			return da > db
		}
		return hits[a].row.Name < hits[b].row.Name
	})
	out := make([]*Row, len(hits))
	for i, h := range hits {
		out[i] = h.row
	}
	return out
}

func Rank(rows []Row, query, kind, category string) []Scored {
	q := strings.ToLower(query)
	var terms []string
	for _, t := range Words(q) {
		if !isStopword(t) {
			terms = append(terms, t)
		}
	}

	deg := DegreeMap(rows)
	var scored []Scored
	for i := range rows {
		row := &rows[i]
		if kind != "" && row.Kind != kind {
			continue
		}
		if category != "" && row.Category != category {
			continue
		}
		name := strings.ToLower(row.Name)
		purpose := strings.ToLower(row.Purpose)
		tags := strings.ToLower(row.Tags)
		usew := strings.ToLower(row.UseWhen)
		notw := strings.ToLower(row.NotWhen)

		var score int64
		vetoed := false
		for _, t := range terms {
			if notw != "" && fhit(notw, t) {
				vetoed = true
				break
			}
			switch {
			case fhit(name, t) || fhit(usew, t):
				score += 3
			case fhit(tags, t):
				score += 2
			case fhit(purpose, t):
				score += 1
			}
		}
		if vetoed || score <= 0 {
			continue
		}
		scored = append(scored, Scored{Score: score, Row: row})
	}
	sortScored(scored, deg)
	return scored
}

func sortScored(scored []Scored, deg map[string]int) {
	sort.SliceStable(scored, func(a, b int) bool {
		if scored[a].Score != scored[b].Score {
			return scored[a].Score > scored[b].Score
		}
		da, db := deg[scored[a].Row.Key], deg[scored[b].Row.Key]
		if da != db {
			return da > db
		}
		return scored[a].Row.Name < scored[b].Row.Name
	})
}
