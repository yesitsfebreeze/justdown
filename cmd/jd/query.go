package main

import (
	"archive/tar"
	"compress/gzip"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	justdown "github.com/yesitsfebreeze/justdown"
)

func toJSON(v any) string {
	var buf strings.Builder
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		panic("jd json output is always serializable: " + err.Error())
	}
	return strings.TrimSuffix(buf.String(), "\n")
}

func csvVec(csv string) []string {
	if csv == "" {
		return []string{}
	}
	return strings.Split(csv, ",")
}

type errorOut struct {
	Schema  string `json:"schema"`
	Error   string `json:"error"`
	Message string `json:"message"`
}

type searchOut struct {
	Schema   string         `json:"schema"`
	Query    string         `json:"query"`
	Results  []searchResult `json:"results"`
	Fallback *fallbackOut   `json:"fallback,omitempty"`
}

type searchResult struct {
	Name        string   `json:"name"`
	Kind        string   `json:"kind"`
	Score       int64    `json:"score"`
	Purpose     string   `json:"purpose"`
	Raw         string   `json:"raw"`
	Source      string   `json:"source"`
	Danger      string   `json:"danger"`
	SideEffects []string `json:"side_effects"`
	Requires    []string `json:"requires"`
}

type fallbackOut struct {
	Reason  string `json:"reason"`
	Name    string `json:"name"`
	Kind    string `json:"kind"`
	Purpose string `json:"purpose"`
	Raw     string `json:"raw"`
}

type getOut struct {
	Schema   string       `json:"schema"`
	Ref      string       `json:"ref"`
	Sections []sectionOut `json:"sections"`
}

type sectionOut struct {
	Kind    string `json:"kind"`
	Content string `json:"content"`
}

type lsOut struct {
	Schema     string       `json:"schema"`
	Categories []lsCategory `json:"categories"`
}

type lsCategory struct {
	Name    string   `json:"name"`
	Members []string `json:"members"`
}

type linksOut struct {
	Schema   string   `json:"schema"`
	Ref      string   `json:"ref"`
	Key      string   `json:"key"`
	Outbound []string `json:"outbound"`
	Inbound  []string `json:"inbound"`
	Fuzzy    []string `json:"fuzzy"`
}

type resolveOut struct {
	Schema   string         `json:"schema"`
	Query    string         `json:"query"`
	Fuzzy    bool           `json:"fuzzy"`
	Resolved *string        `json:"resolved,omitempty"`
	Matches  []resolveMatch `json:"matches"`
}

type resolveMatch struct {
	Key  string `json:"key"`
	Kind string `json:"kind"`
	Path string `json:"path"`
}

type pathOut struct {
	Schema string   `json:"schema"`
	From   string   `json:"from"`
	To     string   `json:"to"`
	Path   []string `json:"path"`
	Length int64    `json:"length"`
}

func emitErr(cfg *config, code, msg string) {
	if cfg.format == formatJSON {
		fmt.Fprintln(os.Stderr, toJSON(errorOut{Schema: "justdown.error/1", Error: code, Message: msg}))
	} else {
		fmt.Fprintf(os.Stderr, "jd: %s\n", msg)
	}
}

var netClient = &http.Client{Timeout: 15 * time.Second}
var snapshotClient = &http.Client{Timeout: 120 * time.Second}

func fetchString(url string) (string, bool) {
	resp, err := netClient.Get(url)
	if err != nil {
		return "", false
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return "", false
	}
	b, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", false
	}
	return string(b), true
}

func commitSidecar(db string) string { return db + ".commit" }
func fpSidecar(db string) string     { return db + ".fp" }

func readStamp(p string) string {
	b, err := os.ReadFile(p)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(b))
}

func isCommit(s string) bool {
	if len(s) != 40 {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F') {
			return false
		}
	}
	return true
}

func cachedBeltRows(cfg *config) []justdown.Row {
	var out []justdown.Row
	sources := cfg.sources()
	for i := len(sources) - 1; i >= 0; i-- {
		s := sources[i]
		cache := beltCachePath(s.slug())
		if cache == "" {
			continue
		}
		var origin string
		if s.git != nil {
			stamp := readStamp(commitSidecar(cache))
			sha := ""
			if f := strings.Fields(stamp); len(f) > 0 && isCommit(f[0]) {
				sha = f[0]
			}
			var base string
			var ok bool
			if sha != "" {
				base, ok = s.git.rawBaseAt(sha)
			} else {
				base, ok = s.git.rawBase()
			}
			if !ok {
				continue
			}
			origin = base
		} else {
			origin = s.dir.dir
		}
		rows, ok := loadStore(cache, justdown.SourceOnline)
		if !ok {
			continue
		}
		for j := range rows {
			rows[j].Origin = origin
		}
		out = append(out, rows...)
	}
	return out
}

func onlineBase(cfg *config, r *justdown.Row) string {
	if r.Origin == "" {
		return cfg.rawBase
	}
	return r.Origin
}

func rawDisplay(cfg *config, r *justdown.Row) string {
	if r.Source.IsLocal() {
		if r.Origin == "" {
			return r.Path
		}
		return r.Origin + "/" + r.Path
	}
	return onlineBase(cfg, r) + "/" + r.Path
}

func loadStore(path string, source justdown.Source) ([]justdown.Row, bool) {
	if _, err := os.Stat(path); err != nil {
		return nil, false
	}
	s, err := justdown.OpenStore(path)
	if err != nil {
		return nil, false
	}
	defer s.Close()
	rows, err := s.LoadRows(source)
	if err != nil {
		return nil, false
	}
	return rows, true
}

func canon(p string) string {
	if r, err := filepath.EvalSymlinks(p); err == nil {
		if a, err := filepath.Abs(r); err == nil {
			return a
		}
	}
	return p
}

func localHomes(cfg *config) []string {
	if nestedEnabled() {
		return justdown.FindJDHomes(cfg.projectDir())
	}
	return []string{cfg.root}
}

func fingerprintFiles(files []string, base string) string {
	if len(files) == 0 {
		return ""
	}
	type entry struct {
		rel   string
		mtime int64
		size  int64
	}
	entries := make([]entry, 0, len(files))
	for _, f := range files {
		rel, err := filepath.Rel(base, f)
		if err != nil {
			rel = f
		}
		rel = filepath.ToSlash(rel)
		var mtime, size int64
		if st, err := os.Stat(f); err == nil {
			mtime = st.ModTime().UnixNano()
			size = st.Size()
		}
		entries = append(entries, entry{rel, mtime, size})
	}
	sort.Slice(entries, func(a, b int) bool {
		if entries[a].rel != entries[b].rel {
			return entries[a].rel < entries[b].rel
		}
		if entries[a].mtime != entries[b].mtime {
			return entries[a].mtime < entries[b].mtime
		}
		return entries[a].size < entries[b].size
	})
	var b strings.Builder
	b.WriteString(cliVersion)
	for _, e := range entries {
		fmt.Fprintf(&b, "|%s|%d|%d", e.rel, e.mtime, e.size)
	}
	return fnvHex(b.String())
}

func localFingerprint(cfg *config) string {
	var files []string
	for _, home := range localHomes(cfg) {
		libdir := filepath.Join(home, cfg.lib)
		if st, err := os.Stat(libdir); err == nil && st.IsDir() {
			justdown.CollectJD(libdir, &files)
		}
	}
	return fingerprintFiles(files, cfg.projectDir())
}

func localFPPath(cfg *config) string {
	dir := localCacheDir()
	if dir == "" {
		return ""
	}
	return filepath.Join(dir, fnvHex(canon(cfg.projectDir()))+".fp")
}

type localState int

const (
	localRebuilt localState = iota
	localCurrent
	localNone
	localFailed
)

func ensureLocalGraph(cfg *config) localState {
	fp := localFingerprint(cfg)
	if fp == "" {
		return localNone
	}
	sidecar := localFPPath(cfg)
	stored := ""
	if sidecar != "" {
		stored = readStamp(sidecar)
	}
	if _, err := os.Stat(cfg.indexPath()); err == nil && stored == fp {
		return localCurrent
	}
	if buildLocalGraph(cfg) {
		if sidecar != "" {
			_ = os.MkdirAll(filepath.Dir(sidecar), 0o755)
			_ = os.WriteFile(sidecar, []byte(fp), 0o644)
		}
		return localRebuilt
	}
	return localFailed
}

func liveLocalRows(cfg *config) []justdown.Row {
	project := cfg.projectDir()
	seen := map[string]bool{}
	var nodes []justdown.Node
	for _, home := range localHomes(cfg) {
		libdir := filepath.Join(home, cfg.lib)
		if st, err := os.Stat(libdir); err != nil || !st.IsDir() {
			continue
		}
		var files []string
		justdown.CollectJD(libdir, &files)
		justdown.SortFiles(files)
		for _, f := range files {
			rel, err := filepath.Rel(project, f)
			if err != nil {
				rel = f
			}
			rel = filepath.ToSlash(rel)
			content, err := os.ReadFile(f)
			if err != nil {
				continue
			}
			node := justdown.Parse(rel, string(content))
			if !seen[node.Key] {
				seen[node.Key] = true
				nodes = append(nodes, node)
			}
		}
	}
	return justdown.RowsFromNodes(nodes, justdown.SourceLocal)
}

type fetchOutcome int

const (
	fetchUpdated fetchOutcome = iota
	fetchUnchanged
	fetchFailed
)

func replaceFile(tmp, dst string) bool {
	if os.Rename(tmp, dst) == nil {
		return true
	}
	in, err := os.ReadFile(tmp)
	if err != nil {
		return false
	}
	if os.WriteFile(dst, in, 0o644) != nil {
		return false
	}
	_ = os.Remove(tmp)
	return true
}

func dirRoots(dir, lib string) []justdown.Root {
	homes := justdown.FindJDHomes(dir)
	if st, err := os.Stat(filepath.Join(dir, lib)); err == nil && st.IsDir() {
		homes = append(homes, dir)
	}
	for i, j := 0, len(homes)-1; i < j; i, j = i+1, j-1 {
		homes[i], homes[j] = homes[j], homes[i] // apply deeper LAST so it wins
	}
	var roots []justdown.Root
	for _, h := range homes {
		l := filepath.Join(h, lib)
		if st, err := os.Stat(l); err == nil && st.IsDir() {
			roots = append(roots, justdown.RootWithBase(l, dir))
		}
	}
	if len(roots) > 0 {
		return roots
	}
	var files []string
	justdown.CollectJD(dir, &files)
	if len(files) == 0 {
		return nil
	}
	return []justdown.Root{justdown.RootWithBase(dir, dir)}
}

func buildSourceGraph(roots []justdown.Root, db string) bool {
	tmp := db + fmt.Sprintf(".tmp%d", os.Getpid())
	_, err := justdown.BuildIndex(tmp, roots, cliVersion)
	ok := err == nil && replaceFile(tmp, db)
	_ = os.Remove(tmp)
	return ok
}

func lsRemoteHead(r *remote) string {
	cmd := exec.Command("git", "ls-remote", r.url, r.gitRef)
	cmd.Env = append(os.Environ(), "GIT_TERMINAL_PROMPT=0")
	out, err := cmd.Output()
	if err != nil {
		return ""
	}
	type pair struct{ sha, name string }
	var lines []pair
	for _, l := range strings.Split(string(out), "\n") {
		f := strings.Fields(l)
		if len(f) >= 2 {
			lines = append(lines, pair{f[0], f[1]})
		}
	}
	for _, want := range []string{
		"refs/tags/" + r.gitRef + "^{}",
		"refs/heads/" + r.gitRef,
		"refs/tags/" + r.gitRef,
	} {
		for _, l := range lines {
			if l.name == want {
				return l.sha
			}
		}
	}
	if len(lines) > 0 {
		return lines[0].sha
	}
	return ""
}

func remoteHead(r *remote) string {
	if isCommit(r.gitRef) {
		return r.gitRef
	}
	if sha := lsRemoteHead(r); sha != "" {
		return sha
	}
	owner, repo, ok := r.githubRepo()
	if !ok {
		return ""
	}
	body, ok := fetchString(fmt.Sprintf(
		"https://api.github.com/repos/%s/%s/commits/%s", owner, repo, r.gitRef))
	if !ok {
		return ""
	}
	idx := strings.Index(body, `"sha"`)
	if idx < 0 {
		return ""
	}
	rest := strings.TrimLeft(body[idx+5:], `: "`)
	end := 0
	for end < len(rest) {
		c := rest[end]
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F') {
			break
		}
		end++
	}
	sha := rest[:end]
	if isCommit(sha) {
		return sha
	}
	return ""
}

func extractTarGz(archive, dest string) bool {
	f, err := os.Open(archive)
	if err != nil {
		return false
	}
	defer f.Close()
	gz, err := gzip.NewReader(f)
	if err != nil {
		return false
	}
	defer gz.Close()
	tr := tar.NewReader(gz)
	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			return true
		}
		if err != nil {
			return false
		}
		name := filepath.FromSlash(hdr.Name)
		if strings.Contains(name, "..") {
			continue
		}
		target := filepath.Join(dest, name)
		switch hdr.Typeflag {
		case tar.TypeDir:
			if os.MkdirAll(target, 0o755) != nil {
				return false
			}
		case tar.TypeReg:
			if os.MkdirAll(filepath.Dir(target), 0o755) != nil {
				return false
			}
			out, err := os.Create(target)
			if err != nil {
				return false
			}
			if _, err := io.Copy(out, tr); err != nil {
				out.Close()
				return false
			}
			out.Close()
		}
	}
}

func refreshRemoteGraph(cfg *config, r *remote, db string) fetchOutcome {
	owner, repo, ok := r.githubRepo()
	if !ok {
		return fetchFailed
	}
	sha := remoteHead(r)
	if sha == "" {
		return fetchFailed
	}
	stamp := sha + " " + cliVersion
	sidecar := commitSidecar(db)
	if _, err := os.Stat(db); err == nil && readStamp(sidecar) == stamp {
		return fetchUnchanged
	}

	pid := os.Getpid()
	tarball := db + fmt.Sprintf(".tar%d", pid)
	srcdir := db + fmt.Sprintf(".src%d", pid)
	cleanup := func() {
		_ = os.Remove(tarball)
		_ = os.RemoveAll(srcdir)
	}
	defer cleanup()

	url := fmt.Sprintf("https://codeload.github.com/%s/%s/tar.gz/%s", owner, repo, sha)
	resp, err := snapshotClient.Get(url)
	if err != nil {
		return fetchFailed
	}
	fetched := false
	if resp.StatusCode == http.StatusOK {
		if out, err := os.Create(tarball); err == nil {
			if _, err := io.Copy(out, resp.Body); err == nil {
				fetched = true
			}
			out.Close()
		}
	}
	resp.Body.Close()
	if !fetched || os.MkdirAll(srcdir, 0o755) != nil {
		return fetchFailed
	}
	if !extractTarGz(tarball, srcdir) {
		return fetchFailed
	}
	root := ""
	if entries, err := os.ReadDir(srcdir); err == nil {
		for _, e := range entries {
			if e.IsDir() {
				root = filepath.Join(srcdir, e.Name())
				break
			}
		}
	}
	if root == "" {
		return fetchFailed
	}
	roots := dirRoots(root, cfg.lib)
	if len(roots) == 0 || !buildSourceGraph(roots, db) {
		return fetchFailed
	}
	_ = os.WriteFile(sidecar, []byte(stamp), 0o644)
	return fetchUpdated
}

func refreshDirGraph(cfg *config, d *dirSource, db string) fetchOutcome {
	roots := dirRoots(d.dir, cfg.lib)
	var files []string
	for _, r := range roots {
		justdown.CollectJD(r.Dir, &files)
	}
	fp := fingerprintFiles(files, d.dir)
	if fp == "" {
		return fetchFailed
	}
	sidecar := fpSidecar(db)
	if _, err := os.Stat(db); err == nil && readStamp(sidecar) == fp {
		return fetchUnchanged
	}
	if buildSourceGraph(roots, db) {
		_ = os.WriteFile(sidecar, []byte(fp), 0o644)
		return fetchUpdated
	}
	return fetchFailed
}

func refreshBelt(cfg *config) []struct {
	slug    string
	outcome fetchOutcome
} {
	var out []struct {
		slug    string
		outcome fetchOutcome
	}
	for _, s := range cfg.sources() {
		db := beltCachePath(s.slug())
		if db == "" {
			continue
		}
		_ = os.MkdirAll(filepath.Dir(db), 0o755)
		_ = os.Remove(db + ".etag") // superseded ETag mechanism — sweep
		var outcome fetchOutcome
		if s.git != nil {
			if _, _, ok := s.git.githubRepo(); !ok {
				continue // non-GitHub hosts stay clone-only / skipped
			}
			outcome = refreshRemoteGraph(cfg, s.git, db)
		} else {
			outcome = refreshDirGraph(cfg, s.dir, db)
		}
		out = append(out, struct {
			slug    string
			outcome fetchOutcome
		}{s.slug(), outcome})
	}
	return out
}

func gather(cfg *config) ([]justdown.Row, int) {
	_ = ensureLocalGraph(cfg)
	local, ok := loadStore(cfg.indexPath(), justdown.SourceLocal)
	if !ok {
		local = liveLocalRows(cfg)
	}
	cached := cachedBeltRows(cfg)

	if len(local) == 0 && len(cached) == 0 {
		emitErr(cfg, "source-unreachable",
			"no repo-local .jd library and no cached belt (run `jd build`)")
		return nil, 4
	}

	seen := map[string]bool{}
	var out []justdown.Row
	for _, row := range append(local, cached...) {
		if !seen[row.Key] {
			seen[row.Key] = true
			out = append(out, row)
		}
	}
	return out, 0
}

func stem(w string) string {
	for _, suf := range []string{
		"izations", "ization", "ations", "ation", "ings", "ing", "ers", "er", "ed", "es", "ly", "s",
	} {
		if len(w) >= len(suf)+2 && strings.HasSuffix(w, suf) {
			return w[:len(w)-len(suf)]
		}
	}
	return w
}

func synonyms(term string) []string {
	switch term {
	case "smaller", "shrink", "compress", "reduce", "size":
		return []string{"resize", "scale", "compress", "smaller", "shrink"}
	case "bigger", "enlarge", "grow", "upscale":
		return []string{"resize", "scale", "bigger", "enlarge"}
	case "make", "create", "new", "generate", "init":
		return []string{"create", "make", "build", "init", "generate"}
	case "remove", "delete", "erase", "clean", "purge":
		return []string{"remove", "delete", "prune", "clean", "rm"}
	case "find", "locate", "lookup", "grep":
		return []string{"search", "find", "grep", "locate"}
	case "show", "display", "view", "print":
		return []string{"show", "list", "display", "print"}
	case "convert", "transform", "change", "transcode":
		return []string{"convert", "transform", "transcode", "change"}
	case "video", "movie", "clip", "film":
		return []string{"video", "media", "ffmpeg", "movie"}
	case "image", "picture", "photo", "img":
		return []string{"image", "picture", "magick", "photo"}
	case "folder", "directory", "dir":
		return []string{"directory", "folder", "dir"}
	case "fast", "speed", "quick", "benchmark":
		return []string{"fast", "speed", "bench", "benchmark"}
	case "secret", "password", "credential", "key":
		return []string{"secret", "credential", "key", "token"}
	}
	return nil
}

func trigrams(s string) map[string]float64 {
	chars := []rune("  " + s + "  ")
	m := map[string]float64{}
	for i := 0; i+2 < len(chars); i++ {
		m[string(chars[i:i+3])]++
	}
	return m
}

func cosine(a, b map[string]float64) float64 {
	if len(a) == 0 || len(b) == 0 {
		return 0
	}
	var dot, na, nb float64
	for k, va := range a {
		if vb, ok := b[k]; ok {
			dot += va * vb
		}
		na += va * va
	}
	for _, vb := range b {
		nb += vb * vb
	}
	if na == 0 || nb == 0 {
		return 0
	}
	return dot / (math.Sqrt(na) * math.Sqrt(nb))
}

func stemTokens(field string) map[string]bool {
	out := map[string]bool{}
	for _, w := range justdown.Words(field) {
		out[stem(w)] = true
	}
	return out
}

func rankSemantic(rows []justdown.Row, query, kind, category string) []justdown.Scored {
	q := strings.ToLower(query)
	var base []string
	for _, t := range justdown.Words(q) {
		stop := false
		for _, s := range justdown.Stopwords {
			if t == s {
				stop = true
				break
			}
		}
		if !stop {
			base = append(base, t)
		}
	}

	var expanded []string
	for _, t := range base {
		for _, cand := range append([]string{t}, synonyms(t)...) {
			s := stem(cand)
			dup := false
			for _, e := range expanded {
				if e == s {
					dup = true
					break
				}
			}
			if !dup {
				expanded = append(expanded, s)
			}
		}
	}
	qtri := trigrams(q)

	deg := justdown.DegreeMap(rows)
	var scored []justdown.Scored
	for i := range rows {
		row := &rows[i]
		if kind != "" && row.Kind != kind {
			continue
		}
		if category != "" && row.Category != category {
			continue
		}
		notw := stemTokens(strings.ToLower(row.NotWhen))
		vetoed := false
		for _, t := range base {
			if notw[stem(t)] {
				vetoed = true
				break
			}
		}
		if vetoed {
			continue
		}
		name := stemTokens(strings.ToLower(row.Name))
		usew := stemTokens(strings.ToLower(row.UseWhen))
		tags := stemTokens(strings.ToLower(row.Tags))
		purpose := stemTokens(strings.ToLower(row.Purpose))

		var score int64
		for _, t := range expanded {
			switch {
			case name[t] || usew[t]:
				score += 3
			case tags[t]:
				score += 2
			case purpose[t]:
				score += 1
			}
		}
		doc := strings.ToLower(row.Name + " " + row.Tags + " " + row.UseWhen + " " + row.Purpose)
		sim := cosine(qtri, trigrams(doc))
		score += int64(math.Round(sim * 6.0))

		if score <= 0 {
			continue
		}
		scored = append(scored, justdown.Scored{Score: score, Row: row})
	}
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
	return scored
}

func cmdSearch(cfg *config, args []string) int {
	mode := ""
	var pos []string
	for i := 0; i < len(args); i++ {
		a := args[i]
		if a == "--mode" {
			i++
			if i >= len(args) {
				emitErr(cfg, "bad-args", "--mode needs a value")
				return 3
			}
			mode = args[i]
		} else if m, ok := strings.CutPrefix(a, "--mode="); ok {
			mode = m
		} else {
			pos = append(pos, a)
		}
	}
	if mode == "" {
		mode = "exact"
	}
	if mode != "exact" && mode != "semantic" {
		emitErr(cfg, "bad-args", fmt.Sprintf("unknown mode: %s (want exact|semantic)", mode))
		return 3
	}

	if len(pos) == 0 || pos[0] == "" {
		emitErr(cfg, "bad-args", "search needs a query")
		return 3
	}
	query := pos[0]
	kind, numS, category := "", "5", ""
	if len(pos) > 1 {
		kind = pos[1]
	}
	if len(pos) > 2 {
		numS = pos[2]
	}
	if len(pos) > 3 {
		category = pos[3]
	}

	if kind != "" && kind != "tool" && kind != "agent" && kind != "knowledge" && kind != "workflow" {
		emitErr(cfg, "bad-args", fmt.Sprintf("unknown kind: %s (want tool|agent|knowledge|workflow)", kind))
		return 3
	}
	allDigits := numS != ""
	for i := 0; i < len(numS); i++ {
		if numS[i] < '0' || numS[i] > '9' {
			allDigits = false
			break
		}
	}
	if !allDigits {
		emitErr(cfg, "bad-args", fmt.Sprintf("num must be a positive integer: %s", numS))
		return 3
	}
	num, err := strconv.Atoi(numS)
	if err != nil || num <= 0 {
		num = 5
	}

	rows, code := gather(cfg)
	if code != 0 {
		return code
	}

	var scored []justdown.Scored
	if mode == "semantic" {
		scored = rankSemantic(rows, query, kind, category)
	} else {
		scored = justdown.Rank(rows, query, kind, category)
	}
	base := cfg.projectDir()
	extra := contentHits(rows, scored, query, kind, category, func(r *justdown.Row) (string, bool) {
		b, err := os.ReadFile(filepath.Join(base, r.Path))
		if err != nil {
			return "", false
		}
		return string(b), true
	})
	for _, row := range extra {
		scored = append(scored, justdown.Scored{Score: 0, Row: row})
	}

	take := len(scored)
	if take > num {
		take = num
	}
	shown := scored[:take]

	if len(shown) == 0 {
		return emitFallback(cfg, query, rows)
	}

	if cfg.format == formatJSON {
		results := make([]searchResult, 0, len(shown))
		for _, s := range shown {
			r := s.Row
			danger := r.Danger
			if danger == "" {
				danger = "none"
			}
			results = append(results, searchResult{
				Name:        r.Name,
				Kind:        r.Kind,
				Score:       s.Score,
				Purpose:     r.Purpose,
				Raw:         rawDisplay(cfg, r),
				Source:      r.Source.Label(),
				Danger:      danger,
				SideEffects: csvVec(r.SideEffects),
				Requires:    csvVec(r.Requires),
			})
		}
		fmt.Println(toJSON(searchOut{Schema: "justdown.search/1", Query: query, Results: results}))
	} else {
		for i, s := range shown {
			r := s.Row
			raw := rawDisplay(cfg, r)
			if r.Source.IsLocal() {
				raw += fmt.Sprintf(" (%s)", r.Source.Label())
			}
			fmt.Printf("%d. %s  [%s]  score %d\n   %s\n   %s\n", i+1, r.Name, r.Kind, s.Score, r.Purpose, raw)
			if r.Danger == "high" || r.Danger == "medium" || r.SideEffects != "" {
				danger := r.Danger
				if danger == "" {
					danger = "none"
				}
				line := fmt.Sprintf("   ⚠ danger=%s", danger)
				if r.SideEffects != "" {
					line += fmt.Sprintf("  effects=%s", r.SideEffects)
				}
				if r.Requires != "" {
					line += fmt.Sprintf("  requires=%s", r.Requires)
				}
				fmt.Println(line)
			}
		}
	}
	return 0
}

func contentHits(rows []justdown.Row, scored []justdown.Scored, query, kind, category string,
	read func(*justdown.Row) (string, bool)) []*justdown.Row {
	terms := justdown.SearchTerms(query)
	if len(terms) == 0 {
		return nil
	}
	hit := map[string]bool{}
	for _, s := range scored {
		hit[s.Row.Key] = true
	}
	var extra []*justdown.Row
	for i := range rows {
		r := &rows[i]
		if !r.Source.IsLocal() || hit[r.Key] ||
			(kind != "" && r.Kind != kind) || (category != "" && r.Category != category) {
			continue
		}
		raw, _ := read(r)
		if matched, _ := justdown.MatchNameContent(r.Path, raw, terms); matched {
			extra = append(extra, r)
		}
	}
	sort.SliceStable(extra, func(a, b int) bool { return extra[a].Name < extra[b].Name })
	return extra
}

const fallbackKey = "help/cht"

func emitFallback(cfg *config, query string, rows []justdown.Row) int {
	var row *justdown.Row
	for i := range rows {
		if rows[i].Key == fallbackKey {
			row = &rows[i]
			break
		}
	}
	switch {
	case row != nil && cfg.format == formatJSON:
		fmt.Println(toJSON(searchOut{
			Schema:  "justdown.search/1",
			Query:   query,
			Results: []searchResult{},
			Fallback: &fallbackOut{
				Reason:  "no-match",
				Name:    row.Name,
				Kind:    row.Kind,
				Purpose: row.Purpose,
				Raw:     rawDisplay(cfg, row),
			},
		}))
	case row != nil:
		fmt.Fprintf(os.Stderr,
			"jd: no library file matched '%s'; cht.sh covers any command or language\n", query)
		fmt.Printf("↳ fallback: %s  [%s]\n   %s\n   get @%s — then run its lang/sheet recipe via just\n",
			row.Name, row.Kind, row.Purpose, row.Key)
	case cfg.format == formatJSON:
		fmt.Println(toJSON(searchOut{Schema: "justdown.search/1", Query: query, Results: []searchResult{}}))
	}
	return 2
}

func basename(path string) string {
	p := strings.TrimSuffix(path, ".jd")
	if i := strings.LastIndexByte(p, '/'); i >= 0 {
		return p[i+1:]
	}
	return p
}

func cmdGet(cfg *config, args []string) int {
	vars := envVars()
	var positional []string
	var flags []string
	for i := 0; i < len(args); i++ {
		a := args[i]
		var pair string
		if a == "--var" {
			i++
			if i >= len(args) {
				emitErr(cfg, "bad-args", "--var needs name=value")
				return 3
			}
			pair = args[i]
		} else if p, ok := strings.CutPrefix(a, "--var="); ok {
			pair = p
		} else if strings.HasPrefix(a, "--") {
			flags = append(flags, a)
			continue
		} else {
			positional = append(positional, a)
			continue
		}
		name, value, ok := strings.Cut(pair, "=")
		if !ok || name == "" {
			emitErr(cfg, "bad-args", fmt.Sprintf("--var wants name=value: %s", pair))
			return 3
		}
		vars[name] = value
	}

	profile, err := parseProfile(flags)
	if err != nil {
		emitErr(cfg, "bad-args", err.Error())
		return 3
	}

	if len(positional) == 0 || positional[0] == "" {
		emitErr(cfg, "bad-args", "get needs a ref")
		return 3
	}
	refr := positional[0]
	if len(positional) > 1 {
		emitErr(cfg, "bad-args",
			"get takes one ref; select output with --human|--agent|--frontmatter|--justfile")
		return 3
	}

	rows, code := gather(cfg)
	if code != 0 {
		return code
	}
	row, code := resolvedOrErr(cfg, rows, refr)
	if code != 0 {
		return code
	}
	body, code := readRowBody(cfg, row)
	if code != 0 {
		return code
	}

	var sections [][2]string
	headers := true
	switch profile {
	case profileJustfile:
		if !justfileKind(row.Kind) {
			emitErr(cfg, "bad-args", fmt.Sprintf(
				"no executable payload: kind '%s' defines types/events, not a recipe — --justfile needs kind tool|workflow",
				row.Kind))
			return 3
		}
		var parts []string
		for _, s := range splitSections(body, "tools") {
			parts = append(parts, s[1])
		}
		sections = [][2]string{{"justfile", strings.Join(parts, "\n")}}
		headers = false
	case profileHuman:
		sections = [][2]string{{"human", stripFrontmatter(body)}}
		headers = false
	case profileFrontmatter:
		sections = splitSections(body, "frontmatter")
	case profileAgent:
		sections = splitSections(body, "frontmatter")
		if prose := proseOnly(body); prose != "" {
			sections = append(sections, [2]string{"prose", prose})
		}
	default:
		sections = splitSections(body, "")
	}

	sections = injectVars(sections, vars)
	if cfg.format == formatJSON {
		secs := make([]sectionOut, len(sections))
		for i, s := range sections {
			secs[i] = sectionOut{Kind: s[0], Content: s[1]}
		}
		fmt.Println(toJSON(getOut{Schema: "justdown.get/1", Ref: refr, Sections: secs}))
	} else {
		for _, s := range sections {
			if headers {
				fmt.Printf("# %s\n%s\n\n", s[0], s[1])
			} else {
				fmt.Println(s[1])
			}
		}
	}
	return 0
}

func readRowBody(cfg *config, row *justdown.Row) (string, int) {
	if strings.HasPrefix(row.Path, "/") || strings.Contains(row.Path, "..") {
		emitErr(cfg, "bad-args", fmt.Sprintf("refusing suspicious path: %s", row.Path))
		return "", 3
	}
	if row.Source.IsLocal() {
		b, err := os.ReadFile(filepath.Join(cfg.projectDir(), row.Path))
		if err != nil {
			emitErr(cfg, "source-unreachable",
				fmt.Sprintf("cannot read %s file: %s", row.Source.Label(), row.Path))
			return "", 4
		}
		return string(b), 0
	}
	base := onlineBase(cfg, row)
	if strings.Contains(base, "://") {
		url := base + "/" + row.Path
		s, ok := fetchString(url)
		if !ok {
			emitErr(cfg, "source-unreachable", fmt.Sprintf("cannot fetch: %s", url))
			return "", 4
		}
		return s, 0
	}
	b, err := os.ReadFile(filepath.Join(base, row.Path))
	if err != nil {
		emitErr(cfg, "source-unreachable", fmt.Sprintf("cannot read %s/%s", base, row.Path))
		return "", 4
	}
	return string(b), 0
}

func renderJustfile(cfg *config, refr string, vars justdown.Vars) (string, int) {
	rows, code := gather(cfg)
	if code != 0 {
		return "", code
	}
	row, code := resolvedOrErr(cfg, rows, refr)
	if code != 0 {
		return "", code
	}
	body, code := readRowBody(cfg, row)
	if code != 0 {
		return "", code
	}
	if !justfileKind(row.Kind) {
		emitErr(cfg, "bad-args", fmt.Sprintf(
			"no executable payload: kind '%s' defines types/events, not a recipe — jd just needs kind tool|workflow",
			row.Kind))
		return "", 3
	}
	var parts []string
	for _, s := range splitSections(body, "tools") {
		parts = append(parts, s[1])
	}
	injected := injectVars([][2]string{{"justfile", strings.Join(parts, "\n")}}, vars)
	var out strings.Builder
	for _, s := range injected {
		out.WriteString(s[1])
	}
	return out.String(), 0
}

type profile int

const (
	profileDefault profile = iota
	profileFrontmatter
	profileHuman
	profileAgent
	profileJustfile
)

func parseProfile(flags []string) (profile, error) {
	prof := profileDefault
	for _, f := range flags {
		var p profile
		switch f {
		case "--frontmatter":
			p = profileFrontmatter
		case "--human":
			p = profileHuman
		case "--agent":
			p = profileAgent
		case "--justfile":
			p = profileJustfile
		default:
			return prof, fmt.Errorf("unknown flag: %s", f)
		}
		if prof != profileDefault {
			return prof, fmt.Errorf("only one output profile (--human|--agent|--frontmatter|--justfile)")
		}
		prof = p
	}
	return prof, nil
}

func justfileKind(kind string) bool {
	return kind == "tool" || kind == "workflow"
}

func stripFrontmatter(body string) string {
	lines := justdown.Lines(body)
	if len(lines) == 0 || lines[0] != "---" {
		return body
	}
	closed := false
	var out []string
	for _, l := range lines[1:] {
		if !closed {
			if l == "---" {
				closed = true
			}
			continue
		}
		out = append(out, l)
	}
	if !closed {
		return body
	}
	for len(out) > 0 && out[0] == "" {
		out = out[1:]
	}
	return strings.Join(out, "\n")
}

func proseOnly(body string) string {
	stripped := stripFrontmatter(body)
	var out []string
	fence := false
	for _, l := range justdown.Lines(stripped) {
		if strings.HasPrefix(l, "```") {
			fence = !fence
			continue
		}
		if !fence {
			out = append(out, l)
		}
	}
	for len(out) > 0 && out[0] == "" {
		out = out[1:]
	}
	for len(out) > 0 && out[len(out)-1] == "" {
		out = out[:len(out)-1]
	}
	return strings.Join(out, "\n")
}

func injectVars(sections [][2]string, vars justdown.Vars) [][2]string {
	if len(vars) == 0 {
		return sections
	}
	out := make([][2]string, len(sections))
	for i, s := range sections {
		out[i] = [2]string{s[0], justdown.Render(s[1], vars)}
	}
	return out
}

func splitSections(body, only string) [][2]string {
	var sections [][2]string
	collectFM := only == "" || only == "frontmatter"

	infm := 0 // 0 before, 1 inside frontmatter, 2 after
	var fmbuf []string
	fence := false
	var blk []string
	plat := justdown.HostPlatform()

	flush := func() {
		if len(blk) == 0 {
			return
		}
		isJust := false
		for _, l := range blk {
			if strings.HasPrefix(l, "```just") {
				isJust = true
				break
			}
		}
		if isJust && (only == "" || only == "tools") {
			var buf []string
			inJust := false
			for _, l := range blk {
				if strings.HasPrefix(l, "```just") {
					inJust = true
					continue
				}
				if inJust && strings.HasPrefix(l, "```") {
					inJust = false
					continue
				}
				if inJust {
					buf = append(buf, l)
				}
			}
			buf = justdown.Platsel(buf, plat)
			sections = append(sections, [2]string{"tools", strings.Join(buf, "\n")})
		} else if !isJust && (only == "" || only == "prose") {
			sections = append(sections, [2]string{"prose", strings.Join(blk, "\n")})
		}
		blk = nil
	}

	for idx, line := range justdown.Lines(body) {
		if idx == 0 && line == "---" {
			infm = 1
			continue
		}
		if infm == 1 && line == "---" {
			infm = 2
			if collectFM {
				sections = append(sections, [2]string{"frontmatter", strings.Join(fmbuf, "\n")})
			}
			continue
		}
		if infm == 1 {
			if collectFM {
				fmbuf = append(fmbuf, line)
			}
			continue
		}
		if strings.HasPrefix(line, "```") {
			fence = !fence
		}
		if !fence && line == "---" {
			flush()
			continue
		}
		if len(blk) != 0 || line != "" {
			blk = append(blk, line)
		}
	}
	flush()
	return sections
}

func cmdLs(cfg *config) int {
	rows, code := gather(cfg)
	if code != 0 {
		return code
	}

	var order []string
	members := map[string][]string{}
	for i := range rows {
		r := &rows[i]
		cat := r.Category
		if cat == "" {
			cat = r.Kind
		}
		if cat == "" {
			cat = "misc"
		}
		if _, ok := members[cat]; !ok {
			order = append(order, cat)
		}
		members[cat] = append(members[cat], r.Name)
	}
	sort.Strings(order)

	if cfg.format == formatJSON {
		categories := make([]lsCategory, len(order))
		for i, c := range order {
			categories[i] = lsCategory{Name: c, Members: members[c]}
		}
		fmt.Println(toJSON(lsOut{Schema: "justdown.ls/1", Categories: categories}))
	} else {
		for _, c := range order {
			var line strings.Builder
			for _, m := range members[c] {
				line.WriteString(" ")
				line.WriteString(m)
			}
			fmt.Printf("%s:%s\n", c, line.String())
		}
	}
	return 0
}

func cmdLinks(cfg *config, args []string) int {
	if len(args) == 0 || args[0] == "" {
		emitErr(cfg, "bad-args", "links needs a ref")
		return 3
	}
	refr := args[0]
	rows, code := gather(cfg)
	if code != 0 {
		return code
	}
	target, code := resolvedOrErr(cfg, rows, refr)
	if code != 0 {
		return code
	}
	key := target.Key
	known := map[string]bool{}
	for i := range rows {
		known[rows[i].Key] = true
	}

	outbound := []string{}
	for _, t := range target.Links {
		if t != key && known[t] {
			outbound = append(outbound, t)
		}
	}
	inbound := []string{}
	for i := range rows {
		r := &rows[i]
		if r.Key == key {
			continue
		}
		for _, d := range r.Links {
			if d == key {
				inbound = append(inbound, r.Name)
				break
			}
		}
	}
	fuzzy := target.Fuzzy
	if fuzzy == nil {
		fuzzy = []string{}
	}

	if cfg.format == formatJSON {
		fmt.Println(toJSON(linksOut{
			Schema: "justdown.links/1", Ref: refr, Key: key,
			Outbound: outbound, Inbound: inbound, Fuzzy: fuzzy,
		}))
	} else {
		for _, o := range outbound {
			fmt.Printf("out  @%s\n", o)
		}
		for _, i := range inbound {
			fmt.Printf("in   %s  (@%s)\n", i, key)
		}
		for _, f := range fuzzy {
			fmt.Printf("fuzz @?%s\n", f)
		}
	}
	return 0
}

func normalizeRef(refr string) string {
	needle := strings.TrimPrefix(refr, "@")
	if i := strings.IndexByte(needle, '#'); i >= 0 {
		needle = needle[:i]
	}
	return needle
}

type resolution struct {
	unique     *justdown.Row
	candidates []string // set on the ambiguous arm
}

func resolveRef(rows []justdown.Row, refr string) resolution {
	needle := normalizeRef(refr)

	dedupKeys := func(matches []*justdown.Row) []*justdown.Row {
		seen := map[string]bool{}
		var out []*justdown.Row
		for _, r := range matches {
			if !seen[r.Key] {
				seen[r.Key] = true
				out = append(out, r)
			}
		}
		return out
	}
	keysOf := func(matches []*justdown.Row) []string {
		keys := make([]string, len(matches))
		for i, r := range matches {
			keys[i] = r.Key
		}
		return keys
	}

	var exact []*justdown.Row
	for i := range rows {
		r := &rows[i]
		if r.Name == needle || r.Key == needle || r.Path == needle {
			exact = append(exact, r)
		}
	}
	exact = dedupKeys(exact)
	if len(exact) == 1 {
		return resolution{unique: exact[0]}
	}
	if len(exact) > 1 {
		return resolution{candidates: keysOf(exact)}
	}

	var byBase []*justdown.Row
	for i := range rows {
		r := &rows[i]
		if basename(r.Path) == needle {
			byBase = append(byBase, r)
		}
	}
	byBase = dedupKeys(byBase)
	if len(byBase) == 1 {
		return resolution{unique: byBase[0]}
	}
	if len(byBase) > 1 {
		return resolution{candidates: keysOf(byBase)}
	}
	return resolution{}
}

func resolvedOrErr(cfg *config, rows []justdown.Row, refr string) (*justdown.Row, int) {
	res := resolveRef(rows, refr)
	if res.unique != nil {
		return res.unique, 0
	}
	if len(res.candidates) > 0 {
		emitErr(cfg, "ambiguous-ref", fmt.Sprintf(
			"'%s' matches %d files: %s — qualify it (e.g. `%s`)",
			refr, len(res.candidates), strings.Join(res.candidates, ", "), res.candidates[0]))
		return nil, 2
	}
	emitErr(cfg, "not-found", fmt.Sprintf("no file: %s", refr))
	return nil, 2
}

func cmdPath(cfg *config, args []string) int {
	if len(args) < 2 || args[0] == "" || args[1] == "" {
		emitErr(cfg, "bad-args", "path needs two refs: jd path <a> <b>")
		return 3
	}
	rows, code := gather(cfg)
	if code != 0 {
		return code
	}
	srcRow, code := resolvedOrErr(cfg, rows, args[0])
	if code != 0 {
		return code
	}
	dstRow, code := resolvedOrErr(cfg, rows, args[1])
	if code != 0 {
		return code
	}
	src, dst := srcRow.Key, dstRow.Key

	known := map[string]bool{}
	for i := range rows {
		known[rows[i].Key] = true
	}
	adj := map[string]map[string]bool{}
	addEdge := func(a, b string) {
		if adj[a] == nil {
			adj[a] = map[string]bool{}
		}
		adj[a][b] = true
	}
	for i := range rows {
		r := &rows[i]
		for _, l := range r.Links {
			if l != r.Key && known[l] {
				addEdge(r.Key, l)
				addEdge(l, r.Key)
			}
		}
	}

	prev := map[string]string{}
	seen := map[string]bool{src: true}
	queue := []string{src}
	hit := src == dst
	for len(queue) > 0 && !hit {
		cur := queue[0]
		queue = queue[1:]
		if cur == dst {
			hit = true
			break
		}
		var ns []string
		for n := range adj[cur] {
			ns = append(ns, n)
		}
		sort.Strings(ns)
		for _, n := range ns {
			if !seen[n] {
				seen[n] = true
				prev[n] = cur
				queue = append(queue, n)
			}
		}
	}
	if !hit {
		hit = seen[dst]
	}

	var chain []string
	if hit {
		chain = []string{dst}
		for cur := dst; cur != src; {
			p := prev[cur]
			chain = append(chain, p)
			cur = p
		}
		for i, j := 0, len(chain)-1; i < j; i, j = i+1, j-1 {
			chain[i], chain[j] = chain[j], chain[i]
		}
	}

	if cfg.format == formatJSON {
		p := chain
		length := int64(len(chain)) - 1
		if chain == nil {
			p = []string{}
			length = -1
		}
		fmt.Println(toJSON(pathOut{Schema: "justdown.path/1", From: src, To: dst, Path: p, Length: length}))
	} else if chain != nil {
		fmt.Println(strings.Join(chain, " → "))
	}

	if chain != nil {
		return 0
	}
	return 2
}

func cmdResolve(cfg *config, args []string) int {
	fuzzy := false
	var pos []string
	for _, a := range args {
		if a == "--fuzzy" {
			fuzzy = true
		} else {
			pos = append(pos, a)
		}
	}
	if len(pos) == 0 || pos[0] == "" {
		emitErr(cfg, "bad-args", "resolve needs a term")
		return 3
	}
	term := strings.TrimPrefix(pos[0], "@")
	if rest, ok := strings.CutPrefix(term, "?"); ok {
		fuzzy = true
		term = rest
	}
	num := 10
	if len(pos) > 1 {
		if n, err := strconv.Atoi(pos[1]); err == nil && n > 0 {
			num = n
		}
	}

	rows, code := gather(cfg)
	if code != 0 {
		return code
	}
	matched, resolved := justdown.ResolveTerm(rows, term, fuzzy, num)
	matches := make([]resolveMatch, len(matched))
	for i, r := range matched {
		matches[i] = resolveMatch{Key: r.Key, Kind: r.Kind, Path: r.Path}
	}

	if cfg.format == formatJSON {
		var resolvedPtr *string
		if resolved != "" {
			resolvedPtr = &resolved
		}
		fmt.Println(toJSON(resolveOut{
			Schema: "justdown.resolve/1", Query: term, Fuzzy: fuzzy,
			Resolved: resolvedPtr, Matches: matches,
		}))
	} else {
		for _, m := range matches {
			leaf := justdown.Leaf(m.Key)
			if fuzzy {
				fmt.Printf("%s  [%s]  @?%s  (%s)\n", leaf, m.Kind, term, m.Key)
			} else {
				fmt.Printf("%s  [%s]  @%s\n", leaf, m.Kind, m.Key)
			}
		}
	}
	return 0
}
