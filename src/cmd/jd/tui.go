package main

import (
	"fmt"
	"os"
	"strings"

	justdown "github.com/yesitsfebreeze/justdown/src"
	"github.com/yesitsfebreeze/justdown/src/editor"
	"golang.org/x/term"
)

// cmdTUI is the real-terminal entrypoint: it owns every fd/raw-mode/signal
// concern (the editor package touches none of it) and hands the rest to
// editor.Run.
func cmdTUI(cfg *config, args []string) int {
	roots := tuiRoots(args)
	if !term.IsTerminal(int(os.Stdin.Fd())) {
		fmt.Fprintln(os.Stderr, "jd: the editor needs an interactive terminal")
		return 1
	}

	old, err := term.MakeRaw(int(os.Stdin.Fd()))
	if err != nil {
		fmt.Fprintln(os.Stderr, "jd: raw mode failed:", err)
		return 1
	}
	defer term.Restore(int(os.Stdin.Fd()), old)

	cols, rows := tuiSize()
	resize := make(chan editor.Size, 1)
	watchResize(resize)

	err = editor.Run(editor.Options{
		Roots:      roots,
		ProjectDir: cfg.projectDir(),
		BeltRows:   func() []justdown.Row { return cachedBeltRows(cfg) },
		Env:        os.Environ(),
		Stdin:      os.Stdin,
		Stdout:     os.Stdout,
		Width:      cols,
		Height:     rows,
		Resize:     resize,
	})
	if err != nil {
		fmt.Fprintln(os.Stderr, "jd:", err)
		return 1
	}
	return 0
}

func tuiSize() (int, int) {
	w, h, err := term.GetSize(int(os.Stdout.Fd()))
	if err != nil || w == 0 {
		return 80, 24
	}
	return w, h
}

func tuiRoots(args []string) []string {
	for _, a := range args {
		if r, ok := strings.CutPrefix(a, "--root="); ok && r != "" {
			return []string{r}
		}
	}
	if r := os.Getenv("JD_ROOT"); r != "" {
		return []string{r}
	}
	if cwd, err := os.Getwd(); err == nil {
		return []string{cwd}
	}
	return []string{"."}
}
