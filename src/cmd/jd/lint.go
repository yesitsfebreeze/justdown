package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	justdown "github.com/yesitsfebreeze/justdown/src"
)

func cmdLint(cfg *config) int {
	libdir := cfg.libDir()
	if st, err := os.Stat(libdir); err != nil || !st.IsDir() {
		fmt.Fprintf(os.Stderr, "jd: no library dir: %s\n", libdir)
		return 1
	}

	var files []string
	justdown.CollectJD(libdir, &files)
	justdown.SortFiles(files)

	var nodes []justdown.Node
	var bodies []string
	for _, f := range files {
		rel, err := filepath.Rel(cfg.root, f)
		if err != nil {
			rel = f
		}
		rel = filepath.ToSlash(rel)
		c, err := os.ReadFile(f)
		if err != nil {
			continue
		}
		nodes = append(nodes, justdown.Parse(rel, string(c)))
		bodies = append(bodies, string(c))
	}

	keys := map[string]bool{}
	keycount := map[string]int{}
	namecount := map[string]int{}
	for i := range nodes {
		n := &nodes[i]
		keys[n.Key] = true
		keycount[n.Key]++
		if n.NameGiven {
			namecount[n.Name]++
		}
	}
	pairs := make([][2]string, len(nodes))
	for i := range nodes {
		pairs[i] = [2]string{nodes[i].Key, nodes[i].Name}
	}
	idx := justdown.BuildNameIndex(pairs)

	errs, warns := 0, 0
	for i := range nodes {
		n := &nodes[i]
		findings := justdown.LintNode(n, bodies[i])
		if n.HasFrontmatter {
			if n.NameGiven && namecount[n.Name] > 1 {
				findings = append(findings, justdown.LintError(fmt.Sprintf("duplicate name: %s", n.Name)))
			}
			if keycount[n.Key] > 1 {
				findings = append(findings, justdown.LintError(fmt.Sprintf("duplicate key: %s", n.Key)))
			}
			for _, l := range n.Links {
				switch form, term := justdown.ClassifyLink(l); form {
				case justdown.LinkFuzzy:
				case justdown.LinkKey:
					if !keys[term] {
						if n.Kind == "knowledge" {
							findings = append(findings, justdown.LintWarn(fmt.Sprintf(
								"unresolved @link: %s (external reference?)", term)))
						} else {
							findings = append(findings, justdown.LintError(fmt.Sprintf("broken @link: %s", term)))
						}
					}
				case justdown.LinkName:
					if _, ok := idx.Resolve(term); ok {
						continue
					}
					if cands := idx.Candidates(term); len(cands) > 1 {
						findings = append(findings, justdown.LintWarn(fmt.Sprintf(
							"ambiguous @link: %s (matches %s)", term, strings.Join(cands, ", "))))
					} else {
						findings = append(findings, justdown.LintWarn(fmt.Sprintf("unresolved @link: %s", term)))
					}
				}
			}
		}
		if len(findings) > 0 {
			fmt.Printf("lint: %s\n", n.Path)
			for _, f := range findings {
				word := "warn"
				if f.IsError() {
					word = "error"
					errs++
				} else {
					warns++
				}
				fmt.Printf("  %s: %s\n", word, f.Message)
			}
		}
	}

	fmt.Printf("\n%d error(s), %d warning(s) across %d file(s)\n", errs, warns, len(nodes))
	if errs > 0 {
		return 1
	}
	return 0
}
