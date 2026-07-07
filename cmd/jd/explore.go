package main

import (
	"bytes"
	"embed"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	justdown "github.com/yesitsfebreeze/justdown"
)

//go:embed editor/index.html editor/app.js editor/style.css
var editorFS embed.FS

const (
	heartbeat   = 5 * time.Second
	feederTTL   = 20 * time.Second
	rewalkEvery = 30 * time.Second
)

var editorDir = func() string {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		return ""
	}
	return filepath.Join(filepath.Dir(file), "editor")
}()

type feeder struct {
	roots    []string
	lastSeen time.Time
}

type exploreState struct {
	mu      sync.Mutex
	feeders map[string]feeder
	index   []string
	rows    []justdown.Row
	gen     atomic.Uint64
	dev     bool
}

func newExploreState(dev bool) *exploreState {
	return &exploreState{feeders: map[string]feeder{}, dev: dev}
}

func (s *exploreState) register(id string, roots []string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	old, ok := s.feeders[id]
	changed := !ok || !equalStrings(old.roots, roots)
	s.feeders[id] = feeder{roots: roots, lastSeen: time.Now()}
	if changed {
		s.gen.Add(1)
	}
}

func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func (s *exploreState) liveRoots() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	before := len(s.feeders)
	for id, f := range s.feeders {
		if time.Since(f.lastSeen) >= feederTTL {
			delete(s.feeders, id)
		}
	}
	if len(s.feeders) != before {
		s.gen.Add(1)
	}
	seen := map[string]bool{}
	var roots []string
	for _, f := range s.feeders {
		for _, r := range f.roots {
			if !seen[r] {
				seen[r] = true
				roots = append(roots, r)
			}
		}
	}
	return roots
}

func cmdExplore(args []string) int {
	port := 3001
	for _, a := range args {
		if p, ok := strings.CutPrefix(a, "--port="); ok {
			if n, err := strconv.Atoi(p); err == nil {
				port = n
			}
		}
	}
	for _, env := range []string{"JD_PORT", "PORT"} {
		if port == 3001 {
			if n, err := strconv.Atoi(os.Getenv(env)); err == nil {
				port = n
			}
		}
	}

	root := os.Getenv("JD_ROOT")
	if root == "" {
		root = homeDir()
	}
	if root == "" {
		root = "."
	}
	roots := []string{root}
	id := fmt.Sprintf("pid-%d", os.Getpid())
	url := fmt.Sprintf("http://localhost:%d", port)
	dev := false
	for _, a := range args {
		if a == "--dev" {
			dev = true
		}
	}

	announced := false
	for {
		listener, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", port))
		switch {
		case err == nil:
			state := newExploreState(dev)
			state.register(id, roots)
			go indexer(state)
			go func() { // the host keeps its own feeder entry fresh directly
				for {
					state.register(id, roots)
					time.Sleep(heartbeat)
				}
			}()
			if !announced {
				fmt.Printf("✺ jd explorer → %s\n", url)
				fmt.Printf("✺ hosting; searching every running jd (this one: %s)\n", root)
				if dev {
					fmt.Printf("✺ dev: serving editor from %s — live reload on\n", editorDir)
				}
				openURL(url)
				announced = true
			}
			srv := &http.Server{Handler: exploreHandler(state)}
			_ = srv.Serve(listener) // blocks; only returns if the server stops
		case isAddrInUse(err):
			fed := postFeed(port, id, roots)
			if fed && !announced {
				fmt.Printf("✺ jd explorer already running → %s\n", url)
				fmt.Printf("✺ feeding: %s\n", root)
				openURL(url)
				announced = true
			}
			time.Sleep(heartbeat)
		default:
			fmt.Fprintf(os.Stderr, "jd: cannot bind 127.0.0.1:%d: %v\n", port, err)
			return 4
		}
	}
}

func isAddrInUse(err error) bool {
	var opErr *net.OpError
	if errors.As(err, &opErr) {
		return strings.Contains(opErr.Err.Error(), "address already in use")
	}
	return false
}

func indexer(state *exploreState) {
	lastGen := ^uint64(0)
	lastWalk := time.Now().Add(-rewalkEvery)
	for {
		roots := state.liveRoots()
		gen := state.gen.Load()
		if gen != lastGen || time.Since(lastWalk) >= rewalkEvery {
			files := walkJD(roots)
			rows := parseRows(files)
			state.mu.Lock()
			state.index = files
			state.rows = rows
			state.mu.Unlock()
			lastGen = gen
			lastWalk = time.Now()
		}
		time.Sleep(2 * time.Second)
	}
}

func exploreHandler(state *exploreState) http.Handler {
	mux := http.NewServeMux()
	page := func(w http.ResponseWriter, ct, name, embeddedPath string) {
		w.Header().Set("Content-Type", ct)
		w.Write([]byte(asset(state.dev, name, embeddedPath)))
	}
	mux.HandleFunc("GET /{$}", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Write([]byte(indexHTML(state.dev)))
	})
	mux.HandleFunc("GET /index.html", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Write([]byte(indexHTML(state.dev)))
	})
	mux.HandleFunc("GET /app.js", func(w http.ResponseWriter, r *http.Request) {
		page(w, "text/javascript; charset=utf-8", "app.js", "editor/app.js")
	})
	mux.HandleFunc("GET /style.css", func(w http.ResponseWriter, r *http.Request) {
		page(w, "text/css; charset=utf-8", "style.css", "editor/style.css")
	})
	mux.HandleFunc("GET /api/livereload", apiLivereload)
	mux.HandleFunc("POST /api/feed", func(w http.ResponseWriter, r *http.Request) { apiFeed(w, r, state) })
	mux.HandleFunc("GET /api/search", func(w http.ResponseWriter, r *http.Request) { apiSearch(w, r, state) })
	mux.HandleFunc("GET /api/resolve", func(w http.ResponseWriter, r *http.Request) { apiResolve(w, r, state) })
	mux.HandleFunc("GET /api/jdtargets", func(w http.ResponseWriter, r *http.Request) { apiJDTargets(w, state) })
	mux.HandleFunc("GET /api/rg", func(w http.ResponseWriter, r *http.Request) { apiRG(w, r, state) })
	mux.HandleFunc("GET /api/file", func(w http.ResponseWriter, r *http.Request) { apiLoad(w, r, state) })
	mux.HandleFunc("POST /api/file", func(w http.ResponseWriter, r *http.Request) { apiSave(w, r, state) })
	mux.HandleFunc("POST /api/reveal", func(w http.ResponseWriter, r *http.Request) { apiReveal(w, r, state) })
	mux.HandleFunc("POST /api/delete", func(w http.ResponseWriter, r *http.Request) { apiDelete(w, r, state) })
	return mux
}

func respondJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	enc := json.NewEncoder(w)
	enc.SetEscapeHTML(false)
	enc.Encode(body)
}

func asset(dev bool, name, embeddedPath string) string {
	if dev && editorDir != "" {
		if b, err := os.ReadFile(filepath.Join(editorDir, name)); err == nil {
			return string(b)
		}
	}
	b, _ := editorFS.ReadFile(embeddedPath)
	return string(b)
}

func indexHTML(dev bool) string {
	html := asset(dev, "index.html", "editor/index.html")
	if !dev {
		return html
	}
	const watch = "<script>(async()=>{let last=null;for(;;){try{const j=await(await fetch('/api/livereload')).json();if(last!==null&&j.mtime!==last)location.reload();last=j.mtime;}catch(e){}await new Promise(r=>setTimeout(r,600));}})();</script>"
	if i := strings.LastIndex(html, "</body>"); i >= 0 {
		return html[:i] + watch + html[i:]
	}
	return html + watch
}

func apiLivereload(w http.ResponseWriter, _ *http.Request) {
	var mtime int64
	for _, f := range []string{"index.html", "app.js", "style.css"} {
		if m := mtimeMS(filepath.Join(editorDir, f)); m > mtime {
			mtime = m
		}
	}
	respondJSON(w, 200, map[string]any{"mtime": mtime})
}

func apiFeed(w http.ResponseWriter, r *http.Request, state *exploreState) {
	var v struct {
		ID    string   `json:"id"`
		Roots []string `json:"roots"`
	}
	if json.NewDecoder(r.Body).Decode(&v) != nil {
		respondJSON(w, 400, map[string]any{"error": "bad body"})
		return
	}
	if v.ID != "" {
		state.register(v.ID, v.Roots)
	}
	respondJSON(w, 200, map[string]any{"ok": true})
}

func apiSearch(w http.ResponseWriter, r *http.Request, state *exploreState) {
	q := strings.TrimSpace(r.URL.Query().Get("q"))
	state.mu.Lock()
	all := append([]string(nil), state.index...)
	state.mu.Unlock()

	files := all
	snippets := map[string]string{}
	if q != "" {
		terms := justdown.SearchTerms(q)
		files = nil
		for _, f := range all {
			raw := ""
			if b, err := os.ReadFile(f); err == nil {
				raw = string(b)
			}
			matched, snippet := justdown.MatchNameContent(displayPath(f), raw, terms)
			if matched {
				files = append(files, f)
				if snippet != "" {
					snippets[f] = snippet
				}
			}
		}
	}

	total := len(files)
	type scoredFile struct {
		path  string
		mtime int64
	}
	scored := make([]scoredFile, len(files))
	for i, f := range files {
		scored[i] = scoredFile{f, mtimeMS(f)}
	}
	sort.SliceStable(scored, func(a, b int) bool { return scored[a].mtime > scored[b].mtime })
	if len(scored) > 50 {
		scored = scored[:50]
	}

	results := make([]map[string]any, 0, len(scored))
	for _, s := range scored {
		display := displayPath(s.path)
		dir := ""
		if p := filepath.Dir(display); p != "." {
			dir = filepath.ToSlash(p)
		}
		entry := map[string]any{
			"path":  s.path,
			"name":  filepath.Base(s.path),
			"dir":   dir,
			"mtime": s.mtime,
		}
		if sn, ok := snippets[s.path]; ok {
			entry["snippet"] = sn
		} else {
			entry["snippet"] = nil
		}
		results = append(results, entry)
	}
	respondJSON(w, 200, map[string]any{"results": results, "total": total, "root": ""})
}

func parseRows(files []string) []justdown.Row {
	var rows []justdown.Row
	for _, f := range files {
		content, err := os.ReadFile(f)
		if err != nil {
			continue
		}
		node := justdown.Parse(filepath.ToSlash(f), string(content))
		rows = append(rows, justdown.RowFromNode(&node, justdown.SourceLocal))
	}
	return rows
}

func matchJSON(r *justdown.Row) map[string]any {
	return map[string]any{"key": r.Key, "kind": r.Kind, "path": r.Path}
}

func apiResolve(w http.ResponseWriter, r *http.Request, state *exploreState) {
	q := strings.ToLower(strings.TrimSpace(r.URL.Query().Get("q")))
	fz := r.URL.Query().Get("fuzzy")
	fuzzy := fz == "1" || fz == "true"
	if q == "" {
		respondJSON(w, 200, map[string]any{"matches": []any{}, "resolved": nil, "fuzzy": fuzzy})
		return
	}
	state.mu.Lock()
	rows := append([]justdown.Row(nil), state.rows...)
	state.mu.Unlock()
	matched, resolved := justdown.ResolveTerm(rows, q, fuzzy, 12)
	matches := make([]map[string]any, len(matched))
	for i, m := range matched {
		matches[i] = matchJSON(m)
	}
	var res any
	if resolved != "" {
		res = resolved
	}
	respondJSON(w, 200, map[string]any{"matches": matches, "resolved": res, "fuzzy": fuzzy})
}

func apiJDTargets(w http.ResponseWriter, state *exploreState) {
	state.mu.Lock()
	rows := append([]justdown.Row(nil), state.rows...)
	state.mu.Unlock()
	targets := map[string]bool{}
	for i := range rows {
		targets[rows[i].Key] = true
		targets[justdown.Leaf(rows[i].Key)] = true
		if rows[i].Name != "" {
			targets[rows[i].Name] = true
		}
	}
	list := make([]string, 0, len(targets))
	for t := range targets {
		list = append(list, t)
	}
	respondJSON(w, 200, map[string]any{"targets": list})
}

func apiRG(w http.ResponseWriter, r *http.Request, state *exploreState) {
	q := strings.TrimSpace(r.URL.Query().Get("q"))
	if q == "" {
		respondJSON(w, 200, map[string]any{"results": []any{}, "total": 0})
		return
	}
	roots := state.liveRoots()
	args := []string{"--json", "-F", "--max-count", "50", "-g", "*.jd", "--", q}
	args = append(args, roots...)
	cmd := exec.Command("rg", args...)
	out, err := cmd.Output()
	if err != nil && len(out) == 0 {
		if _, ok := err.(*exec.ExitError); !ok {
			respondJSON(w, 200, map[string]any{
				"results": []any{}, "total": 0, "error": "ripgrep (rg) not found on PATH"})
			return
		}
	}

	var results []map[string]any
	seen := map[string]bool{}
	for _, line := range bytes.Split(out, []byte("\n")) {
		var v struct {
			Type string `json:"type"`
			Data struct {
				Path struct {
					Text string `json:"text"`
				} `json:"path"`
				LineNumber int64 `json:"line_number"`
				Lines      struct {
					Text string `json:"text"`
				} `json:"lines"`
				Submatches []struct {
					Start int64 `json:"start"`
				} `json:"submatches"`
			} `json:"data"`
		}
		if json.Unmarshal(line, &v) != nil || v.Type != "match" || v.Data.Path.Text == "" {
			continue
		}
		key := fmt.Sprintf("%s:%d", v.Data.Path.Text, v.Data.LineNumber)
		if seen[key] {
			continue
		}
		seen[key] = true
		snippet := strings.TrimSpace(strings.TrimRight(v.Data.Lines.Text, "\n\r"))
		if rn := []rune(snippet); len(rn) > 160 {
			snippet = string(rn[:160])
		}
		var col int64
		if len(v.Data.Submatches) > 0 {
			col = v.Data.Submatches[0].Start
		}
		display := displayPath(v.Data.Path.Text)
		dir := ""
		if p := filepath.Dir(display); p != "." {
			dir = filepath.ToSlash(p)
		}
		results = append(results, map[string]any{
			"path": v.Data.Path.Text,
			"name": filepath.Base(v.Data.Path.Text),
			"dir":  dir,
			"line": v.Data.LineNumber,
			"col":  col,
			"text": snippet,
		})
		if len(results) >= 200 {
			break
		}
	}
	if results == nil {
		results = []map[string]any{}
	}
	respondJSON(w, 200, map[string]any{"results": results, "total": len(results)})
}

func apiLoad(w http.ResponseWriter, r *http.Request, state *exploreState) {
	p, ok := safePath(state, r.URL.Query().Get("path"))
	if !ok {
		respondJSON(w, 404, map[string]any{"error": "File not found"})
		return
	}
	b, err := os.ReadFile(p)
	if err != nil {
		respondJSON(w, 404, map[string]any{"error": "File not found"})
		return
	}
	respondJSON(w, 200, map[string]any{"content": string(b), "path": p})
}

func apiSave(w http.ResponseWriter, r *http.Request, state *exploreState) {
	p, ok := safePath(state, r.URL.Query().Get("path"))
	if !ok {
		respondJSON(w, 500, map[string]any{"error": "Access denied"})
		return
	}
	var v struct {
		Content string `json:"content"`
	}
	if json.NewDecoder(r.Body).Decode(&v) != nil {
		respondJSON(w, 500, map[string]any{"error": "bad body"})
		return
	}
	_, statErr := os.Stat(p)
	isNew := statErr != nil
	_ = os.MkdirAll(filepath.Dir(p), 0o755)
	if err := os.WriteFile(p, []byte(v.Content), 0o644); err != nil {
		respondJSON(w, 500, map[string]any{"error": err.Error()})
		return
	}
	respondJSON(w, 200, map[string]any{"success": true, "isNew": isNew, "path": p})
}

func apiReveal(w http.ResponseWriter, r *http.Request, state *exploreState) {
	if p, ok := safePath(state, r.URL.Query().Get("path")); ok {
		revealInFileManager(p)
		respondJSON(w, 200, map[string]any{"ok": true})
		return
	}
	respondJSON(w, 400, map[string]any{"error": "Access denied"})
}

func apiDelete(w http.ResponseWriter, r *http.Request, state *exploreState) {
	p, ok := safePath(state, r.URL.Query().Get("path"))
	if !ok {
		respondJSON(w, 400, map[string]any{"error": "Access denied"})
		return
	}
	if err := os.Remove(p); err != nil {
		respondJSON(w, 400, map[string]any{"error": err.Error()})
		return
	}
	respondJSON(w, 200, map[string]any{"ok": true})
}

func walkJD(roots []string) []string {
	skip := map[string]bool{
		"node_modules": true, ".git": true, "target": true,
		".Trash": true, ".cache": true, "Caches": true,
	}
	seen := map[string]bool{}
	var out []string
	stack := append([]string(nil), roots...)
	for len(stack) > 0 {
		dir := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		entries, err := os.ReadDir(dir)
		if err != nil {
			continue
		}
		for _, entry := range entries {
			path := filepath.Join(dir, entry.Name())
			if entry.Type().IsDir() {
				if !skip[entry.Name()] {
					stack = append(stack, path)
				}
			} else if entry.Type().IsRegular() &&
				strings.HasSuffix(entry.Name(), ".jd") && entry.Name() != ".jd" && !seen[path] {
				seen[path] = true
				out = append(out, path)
			}
		}
	}
	return out
}

func safePath(state *exploreState, p string) (string, bool) {
	roots := state.liveRoots()
	joined := p
	if !filepath.IsAbs(p) {
		if len(roots) == 0 {
			return "", false
		}
		joined = filepath.Join(roots[0], p)
	}
	clean := filepath.Clean(joined)
	for _, r := range roots {
		if clean == r || strings.HasPrefix(clean, r+string(filepath.Separator)) {
			return clean, true
		}
	}
	return "", false
}

func displayPath(p string) string {
	s := filepath.ToSlash(p)
	if home := homeDir(); home != "" {
		if rest, ok := strings.CutPrefix(s, filepath.ToSlash(home)); ok {
			return "~" + rest
		}
	}
	return s
}

func mtimeMS(p string) int64 {
	st, err := os.Stat(p)
	if err != nil {
		return 0
	}
	return st.ModTime().UnixMilli()
}

func homeDir() string {
	if h := os.Getenv("HOME"); h != "" {
		return h
	}
	return os.Getenv("USERPROFILE")
}

func openURL(url string) {
	for _, attempt := range [][]string{
		{"xdg-open", url},
		{"wslview", url},
		{"open", url},
		{"cmd.exe", "/c", "start", url},
	} {
		cmd := exec.Command(attempt[0], attempt[1:]...)
		if cmd.Start() == nil {
			return
		}
	}
}

func revealInFileManager(p string) {
	dir := filepath.Dir(p)
	for _, attempt := range [][]string{
		{"open", "-R", p},
		{"explorer.exe", "/select," + p},
		{"xdg-open", dir},
	} {
		cmd := exec.Command(attempt[0], attempt[1:]...)
		if cmd.Start() == nil {
			return
		}
	}
}

func postFeed(port int, id string, roots []string) bool {
	body, _ := json.Marshal(map[string]any{"id": id, "roots": roots})
	client := &http.Client{Timeout: 2 * time.Second}
	u := url.URL{Scheme: "http", Host: fmt.Sprintf("127.0.0.1:%d", port), Path: "/api/feed"}
	resp, err := client.Post(u.String(), "application/json", bytes.NewReader(body))
	if err != nil {
		return false
	}
	defer resp.Body.Close()
	return resp.StatusCode == 200
}
