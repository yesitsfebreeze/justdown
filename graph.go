package justdown

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type Root struct {
	Dir     string
	RelBase string
}

func NewRoot(dir string) Root { return Root{Dir: dir, RelBase: dir} }

func RootWithBase(dir, relBase string) Root { return Root{Dir: dir, RelBase: relBase} }

var pruneDirs = []string{
	".git",
	".git-fs",
	"node_modules",
	"target",
	"dist",
	"build",
	"vendor",
	".worktrees",
}

const maxHomeDepth = 8

func FindJDHomes(start string) []string {
	var homes []string
	walkHomes(start, 0, &homes)
	sort.SliceStable(homes, func(a, b int) bool {
		ca := strings.Count(filepath.ToSlash(homes[a]), "/")
		cb := strings.Count(filepath.ToSlash(homes[b]), "/")
		if ca != cb {
			return ca > cb
		}
		return homes[a] < homes[b]
	})
	return homes
}

func walkHomes(dir string, depth int, out *[]string) {
	if depth > maxHomeDepth {
		return
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return
	}
	for _, entry := range entries {
		if !entry.Type().IsDir() {
			continue
		}
		name := entry.Name()
		path := filepath.Join(dir, name)
		if name == ".jd" {
			*out = append(*out, path) // a home — record it, don't descend into it
			continue
		}
		skip := false
		for _, p := range pruneDirs {
			if name == p {
				skip = true
				break
			}
		}
		if skip {
			continue
		}
		walkHomes(path, depth+1, out)
	}
}

func CollectJD(dir string, out *[]string) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return
	}
	for _, entry := range entries {
		path := filepath.Join(dir, entry.Name())
		info, err := os.Stat(path)
		if err != nil {
			continue
		}
		if info.IsDir() {
			CollectJD(path, out)
		} else if name := entry.Name(); strings.HasSuffix(name, ".jd") && name != ".jd" {
			*out = append(*out, path)
		}
	}
}

func SortFiles(files []string) {
	sort.Strings(files)
}

func relSlash(path, base string) string {
	rel, err := filepath.Rel(base, path)
	if err != nil || strings.HasPrefix(rel, "..") {
		rel = path
	}
	return filepath.ToSlash(rel)
}

func BuildIndex(storePath string, roots []Root, producer string) (int, error) {
	byKey := map[string]Node{}
	for _, root := range roots {
		var files []string
		CollectJD(root.Dir, &files)
		SortFiles(files)
		for _, f := range files {
			rel := relSlash(f, root.RelBase)
			content, err := os.ReadFile(f)
			if err != nil {
				continue // unreadable file: skip (host can pre-validate)
			}
			node := Parse(rel, string(content))
			byKey[node.Key] = node // later root overrides earlier
		}
	}

	keys := make([]string, 0, len(byKey))
	for k := range byKey {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	nodes := make([]Node, len(keys))
	for i, k := range keys {
		nodes[i] = byKey[k]
	}
	if err := BuildStore(storePath, nodes, producer); err != nil {
		return 0, err
	}
	return len(nodes), nil
}
