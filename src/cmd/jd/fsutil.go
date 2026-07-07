package main

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	justdown "github.com/yesitsfebreeze/justdown/src"
)

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
