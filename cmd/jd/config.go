package main

import (
	"fmt"
	"hash/fnv"
	"os"
	"path/filepath"
	"strings"

	justdown "github.com/yesitsfebreeze/justdown"
)

type format int

const (
	formatText format = iota
	formatJSON
)

type config struct {
	root    string
	lib     string
	index   string
	repo    string
	gitRef  string
	rawBase string
	format  format
}

type remote struct {
	url    string
	gitRef string
	slug   string
}

type dirSource struct {
	dir  string
	slug string
}

type beltSource struct {
	git *remote
	dir *dirSource
}

func (s beltSource) slug() string {
	if s.git != nil {
		return s.git.slug
	}
	return s.dir.slug
}

func (r *remote) githubRepo() (string, string, bool) {
	rest := r.url
	if _, after, ok := strings.Cut(rest, "://"); ok {
		rest = after
	}
	host, path, ok := strings.Cut(rest, "/")
	if !ok || host != "github.com" {
		return "", "", false
	}
	segs := strings.Split(strings.TrimSuffix(path, ".git"), "/")
	if len(segs) < 2 || segs[0] == "" || segs[1] == "" {
		return "", "", false
	}
	return segs[0], segs[1], true
}

func (r *remote) rawBaseAt(at string) (string, bool) {
	owner, repo, ok := r.githubRepo()
	if !ok {
		return "", false
	}
	return fmt.Sprintf("https://raw.githubusercontent.com/%s/%s/%s", owner, repo, at), true
}

func (r *remote) rawBase() (string, bool) { return r.rawBaseAt(r.gitRef) }

func slugify(s string) string {
	var b strings.Builder
	for _, c := range s {
		if c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' ||
			c == '-' || c == '_' || c == '.' {
			b.WriteRune(c)
		} else {
			b.WriteByte('-')
		}
	}
	return b.String()
}

func readJDConfig(path string) []string {
	text, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	var out []string
	for _, l := range strings.Split(string(text), "\n") {
		l, _, _ = strings.Cut(l, "#")
		l = strings.TrimSpace(l)
		if l != "" {
			out = append(out, l)
		}
	}
	return out
}

func isDirEntry(entry string) bool {
	return strings.HasPrefix(entry, "/") ||
		entry == "~" || strings.HasPrefix(entry, "~/") ||
		entry == "." || strings.HasPrefix(entry, "./") ||
		entry == ".." || strings.HasPrefix(entry, "../")
}

func fnvHex(s string) string {
	h := fnv.New64a()
	h.Write([]byte(s))
	return fmt.Sprintf("%016x", h.Sum64())
}

func parseDir(entry, base string) dirSource {
	var path string
	if rest, ok := strings.CutPrefix(entry, "~"); ok {
		path = filepath.Join(os.Getenv("HOME"), strings.TrimPrefix(rest, "/"))
	} else if filepath.IsAbs(entry) {
		path = entry
	} else {
		path = filepath.Join(base, entry)
	}
	abs := path
	if r, err := filepath.EvalSymlinks(path); err == nil {
		if a, err := filepath.Abs(r); err == nil {
			abs = a
		}
	}
	name := filepath.Base(abs)
	if name == "" || name == "." || name == string(filepath.Separator) {
		name = "dir"
	}
	h := fnv.New64a()
	h.Write([]byte(abs))
	return dirSource{
		slug: fmt.Sprintf("dir-%s-%08x", slugify(name), uint32(h.Sum64())),
		dir:  abs,
	}
}

func parseSource(entry, defaultRef, base string) (beltSource, bool) {
	entry = strings.TrimSpace(entry)
	if entry == "" {
		return beltSource{}, false
	}
	if isDirEntry(entry) {
		d := parseDir(entry, base)
		return beltSource{dir: &d}, true
	}
	spec, gitRef := entry, defaultRef
	if !strings.Contains(entry, "://") {
		if i := strings.LastIndexByte(entry, '@'); i >= 0 {
			r := entry[i+1:]
			if r != "" && !strings.Contains(r, "/") {
				spec, gitRef = entry[:i], r
			}
		}
	}
	var rem remote
	if strings.Contains(spec, "://") {
		trimmed := strings.TrimSuffix(spec, ".git")
		segs := strings.Split(trimmed, "/")
		tail := segs
		if len(segs) > 2 {
			tail = segs[len(segs)-2:]
		}
		rem = remote{url: spec, gitRef: gitRef, slug: slugify(strings.Join(tail, "-"))}
	} else {
		rem = remote{
			url:    fmt.Sprintf("https://github.com/%s.git", spec),
			gitRef: gitRef,
			slug:   slugify(strings.ReplaceAll(spec, "/", "-")),
		}
	}
	return beltSource{git: &rem}, true
}

func envOr(key, def string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return def
}

func isHome(dir, lib string) bool {
	if st, err := os.Stat(filepath.Join(dir, lib)); err == nil && st.IsDir() {
		return true
	}
	if st, err := os.Stat(filepath.Join(dir, "graph.db")); err == nil && !st.IsDir() {
		return true
	}
	return false
}

func resolveRoot(lib string) string {
	if r := os.Getenv("JUSTDOWN_ROOT"); r != "" {
		return r
	}
	cwd, err := os.Getwd()
	if err != nil {
		cwd = "."
	}
	home := os.Getenv("HOME")
	for ancestor := cwd; ; {
		if home != "" && ancestor == home {
			break
		}
		jd := filepath.Join(ancestor, ".jd")
		if isHome(jd, lib) {
			return jd
		}
		if filepath.Base(ancestor) == ".jd" && isHome(ancestor, lib) {
			return ancestor
		}
		parent := filepath.Dir(ancestor)
		if parent == ancestor {
			break
		}
		ancestor = parent
	}
	return filepath.Join(cwd, ".jd")
}

func configFromEnv() config {
	lib := envOr("JUSTDOWN_LIB", "library")
	root := resolveRoot(lib)
	index := envOr("JUSTDOWN_INDEX", "remote-graph.db")
	repo := envOr("JUSTDOWN_REPO", "yesitsfebreeze/justdown")
	branch := envOr("JUSTDOWN_BRANCH", "main")
	ref := envOr("JUSTDOWN_REF", branch)
	rawBase := envOr("JUSTDOWN_RAW_BASE",
		fmt.Sprintf("https://raw.githubusercontent.com/%s/%s", repo, ref))
	return config{
		root:    root,
		lib:     lib,
		index:   index,
		repo:    repo,
		gitRef:  ref,
		rawBase: rawBase,
		format:  formatText,
	}
}

func envVars() justdown.Vars {
	vars := justdown.Vars{}
	for _, kv := range os.Environ() {
		k, v, _ := strings.Cut(kv, "=")
		if name, ok := strings.CutPrefix(k, "JUSTDOWN_VAR_"); ok && name != "" {
			vars[strings.ToLower(name)] = v
		}
	}
	return vars
}

func (c *config) libDir() string { return filepath.Join(c.root, c.lib) }

func (c *config) cacheDir() string { return c.root }

func (c *config) indexPath() string {
	if filepath.IsAbs(c.index) {
		return c.index
	}
	return filepath.Join(c.cacheDir(), c.index)
}

func nestedEnabled() bool {
	switch os.Getenv("JUSTDOWN_NESTED") {
	case "0", "false", "off":
		return false
	}
	return true
}

func (c *config) projectDir() string {
	parent := filepath.Dir(c.root)
	if parent == c.root {
		return c.root
	}
	return parent
}

func cacheRoot() string {
	if d := os.Getenv("XDG_CACHE_HOME"); d != "" {
		return filepath.Join(d, "justdown")
	}
	if h := os.Getenv("HOME"); h != "" {
		return filepath.Join(h, ".cache", "justdown")
	}
	return ""
}

func beltCachePath(slug string) string {
	root := cacheRoot()
	if root == "" {
		return ""
	}
	return filepath.Join(root, "belt", slug+".db")
}

func localCacheDir() string {
	root := cacheRoot()
	if root == "" {
		return ""
	}
	return filepath.Join(root, "local")
}

func (c *config) sources() []beltSource {
	var entries []string
	if env := os.Getenv("JUSTDOWN_REPOS"); env != "" {
		for _, s := range strings.FieldsFunc(env, func(r rune) bool {
			return r == ',' || r == '\n' || r == ' ' || r == '\t'
		}) {
			if s != "" {
				entries = append(entries, s)
			}
		}
	} else {
		entries = readJDConfig(filepath.Join(c.cacheDir(), ".jdconfig"))
	}

	base := c.projectDir()
	var belt []beltSource
	for _, e := range entries {
		s, ok := parseSource(e, c.gitRef, base)
		if !ok {
			continue
		}
		kept := belt[:0]
		for _, x := range belt {
			if x.slug() != s.slug() {
				kept = append(kept, x)
			}
		}
		belt = append(kept, s) // last wins
	}
	if len(belt) == 0 {
		belt = append(belt, beltSource{git: &remote{
			url:    fmt.Sprintf("https://github.com/%s.git", c.repo),
			gitRef: c.gitRef,
			slug:   slugify(strings.ReplaceAll(c.repo, "/", "-")),
		}})
	}
	return belt
}
