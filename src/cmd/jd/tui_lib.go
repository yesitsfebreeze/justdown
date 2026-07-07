package main

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	justdown "github.com/yesitsfebreeze/justdown/src"
)

// library is the in-process index the editor searches and resolves against.
// Editable files come from a filesystem walk of the roots (cwd by default);
// read-only capability rows (local library + already-cached remote / plugin
// belts) drive @link resolution and completion.
type library struct {
	cfg   *config
	roots []string

	mu      sync.Mutex
	files   []string        // absolute paths of editable .jd files
	rows    []justdown.Row  // local + cached-remote rows for resolve/targets
	targets map[string]bool // lowercased link targets (key / leaf / name)
}

func newLibrary(cfg *config, roots []string) *library {
	return &library{cfg: cfg, roots: roots, targets: map[string]bool{}}
}

// reindex rebuilds the file list and row set. Safe to call from a goroutine.
func (l *library) reindex() {
	files := walkJD(l.roots)
	sort.Strings(files)
	rows := parseRows(files)

	// merge already-cached remote / plugin belt rows (no network) so @links can
	// resolve to capabilities that live outside the working tree.
	rows = append(rows, cachedBeltRows(l.cfg)...)

	targets := map[string]bool{}
	for i := range rows {
		k := strings.ToLower(rows[i].Key)
		if k != "" {
			targets[k] = true
			targets[strings.ToLower(justdown.Leaf(rows[i].Key))] = true
		}
		if rows[i].Name != "" {
			targets[strings.ToLower(rows[i].Name)] = true
		}
	}

	l.mu.Lock()
	l.files = files
	l.rows = rows
	l.targets = targets
	l.mu.Unlock()
}

// watch reindexes in the background on a fixed cadence so external edits and
// newly-cached belts show up without a restart.
func (l *library) watch(stop <-chan struct{}, changed func()) {
	t := time.NewTicker(30 * time.Second)
	defer t.Stop()
	for {
		select {
		case <-stop:
			return
		case <-t.C:
			l.reindex()
			if changed != nil {
				changed()
			}
		}
	}
}

type finderHit struct {
	path    string
	name    string
	dir     string
	snippet string
	mtime   int64
}

// search reproduces the explorer's file finder: empty query lists everything by
// recency; otherwise every whitespace term must match the display path as a
// subsequence OR the raw content as a substring. Results are mtime-desc, capped.
func (l *library) search(query string, limit int) []finderHit {
	l.mu.Lock()
	files := append([]string(nil), l.files...)
	l.mu.Unlock()

	q := strings.TrimSpace(query)
	type scored struct {
		hit   finderHit
		mtime int64
	}
	var scoredHits []scored
	if q == "" {
		for _, f := range files {
			scoredHits = append(scoredHits, scored{l.hitFor(f, ""), mtimeMS(f)})
		}
	} else {
		terms := justdown.SearchTerms(q)
		for _, f := range files {
			raw := ""
			if b, err := os.ReadFile(f); err == nil {
				raw = string(b)
			}
			matched, snippet := justdown.MatchNameContent(displayPath(f), raw, terms)
			if matched {
				scoredHits = append(scoredHits, scored{l.hitFor(f, snippet), mtimeMS(f)})
			}
		}
	}
	sort.SliceStable(scoredHits, func(a, b int) bool { return scoredHits[a].mtime > scoredHits[b].mtime })
	if limit > 0 && len(scoredHits) > limit {
		scoredHits = scoredHits[:limit]
	}
	out := make([]finderHit, len(scoredHits))
	for i, s := range scoredHits {
		out[i] = s.hit
	}
	return out
}

type grepHit struct {
	path string
	line int // 1-based
	col  int // 1-based rune column
	text string
}

// grep does an in-process literal content search across editable files (no rg
// dependency). Smart-case: case-insensitive unless the query has an uppercase
// rune. Deduped by path:line, capped.
func (l *library) grep(query string, limit int) []grepHit {
	q := strings.TrimSpace(query)
	if q == "" {
		return nil
	}
	l.mu.Lock()
	files := append([]string(nil), l.files...)
	l.mu.Unlock()

	sensitive := q != strings.ToLower(q) // smart-case
	var out []grepHit
	for _, f := range files {
		b, err := os.ReadFile(f)
		if err != nil {
			continue
		}
		for i, line := range strings.Split(string(b), "\n") {
			spans := matchesInLine([]rune(line), q, sensitive)
			if len(spans) == 0 {
				continue
			}
			snippet := strings.TrimSpace(line)
			if len([]rune(snippet)) > 160 {
				snippet = string([]rune(snippet)[:160])
			}
			out = append(out, grepHit{path: f, line: i + 1, col: spans[0][0] + 1, text: snippet})
			if limit > 0 && len(out) >= limit {
				return out
			}
		}
	}
	return out
}

func (l *library) hitFor(path, snippet string) finderHit {
	display := displayPath(path)
	dir := ""
	if p := filepath.Dir(display); p != "." {
		dir = filepath.ToSlash(p)
	}
	return finderHit{
		path:    path,
		name:    filepath.Base(path),
		dir:     dir,
		snippet: snippet,
		mtime:   mtimeMS(path),
	}
}

// resolve returns ranked @link matches for the completion popup and follow.
func (l *library) resolve(term string, fuzzy bool, limit int) ([]*justdown.Row, string) {
	l.mu.Lock()
	rows := append([]justdown.Row(nil), l.rows...)
	l.mu.Unlock()
	return justdown.ResolveTerm(rows, strings.ToLower(strings.TrimSpace(term)), fuzzy, limit)
}

func (l *library) isTarget(token string) bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.targets[strings.ToLower(token)]
}

// localPathForRow maps a resolved row back to an editable file when it lives in
// the working tree; ok is false for remote/cached capabilities.
func (l *library) localPathForRow(r *justdown.Row) (string, bool) {
	if r == nil || !r.Source.IsLocal() {
		return "", false
	}
	l.mu.Lock()
	files := append([]string(nil), l.files...)
	l.mu.Unlock()
	leaf := justdown.Leaf(r.Key)
	// r.Path is relative to the project dir; match by suffix against the walk.
	rel := filepath.ToSlash(r.Path)
	for _, f := range files {
		fs := filepath.ToSlash(f)
		if strings.HasSuffix(fs, rel) || strings.HasSuffix(strings.TrimSuffix(fs, ".jd"), "/"+leaf) {
			return f, true
		}
	}
	// fall back to project-relative join.
	p := filepath.Join(l.cfg.projectDir(), r.Path)
	if _, err := os.Stat(p); err == nil {
		return p, true
	}
	return "", false
}
