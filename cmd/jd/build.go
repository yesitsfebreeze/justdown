package main

import (
	"fmt"
	"os"
	"path/filepath"

	justdown "github.com/yesitsfebreeze/justdown"
)

func buildLocalGraph(cfg *config) bool {
	project := cfg.projectDir()
	homes := localHomes(cfg)
	for i, j := 0, len(homes)-1; i < j; i, j = i+1, j-1 {
		homes[i], homes[j] = homes[j], homes[i] // deeper LAST so it wins
	}

	var roots []justdown.Root
	for _, home := range homes {
		libdir := filepath.Join(home, cfg.lib)
		if st, err := os.Stat(libdir); err == nil && st.IsDir() {
			roots = append(roots, justdown.RootWithBase(libdir, project))
		}
	}
	if len(roots) == 0 {
		return false
	}

	out := cfg.indexPath()
	if err := os.MkdirAll(filepath.Dir(out), 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "jd: cannot create cache dir %s: %v\n", filepath.Dir(out), err)
		return false
	}
	if _, err := justdown.BuildIndex(out, roots, cliVersion); err != nil {
		fmt.Fprintf(os.Stderr, "jd: failed to write store %s: %v\n", out, err)
		return false
	}
	return true
}

func cmdBuild(cfg *config, _ []string) int {
	switch ensureLocalGraph(cfg) {
	case localRebuilt:
		fmt.Fprintln(os.Stderr, "jd: local graph rebuilt")
	case localCurrent:
		fmt.Fprintln(os.Stderr, "jd: local graph up to date")
	case localNone:
		fmt.Fprintln(os.Stderr, "jd: no local .jd library to build")
	case localFailed:
		fmt.Fprintln(os.Stderr, "jd: local graph build failed")
	}

	for _, r := range refreshBelt(cfg) {
		switch r.outcome {
		case fetchUpdated:
			fmt.Fprintf(os.Stderr, "jd: %s graph rebuilt\n", r.slug)
		case fetchUnchanged:
			fmt.Fprintf(os.Stderr, "jd: %s up to date\n", r.slug)
		case fetchFailed:
			fmt.Fprintf(os.Stderr, "jd: could not refresh %s\n", r.slug)
		}
	}
	return 0
}
