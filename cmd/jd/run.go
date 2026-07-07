package main

import (
	"fmt"
	"os"
	"os/exec"
	"strings"
)

func cmdJust(cfg *config, args []string) int {
	vars := envVars()
	var rest []string
	for i := 0; i < len(args); i++ {
		a := args[i]
		var pair string
		if a == "--var" {
			i++
			if i >= len(args) {
				fmt.Fprintln(os.Stderr, "jd: --var needs name=value")
				return 3
			}
			pair = args[i]
		} else if p, ok := strings.CutPrefix(a, "--var="); ok {
			pair = p
		} else {
			rest = append(rest, a)
			continue
		}
		name, value, ok := strings.Cut(pair, "=")
		if !ok || name == "" {
			fmt.Fprintf(os.Stderr, "jd: --var wants name=value: %s\n", pair)
			return 3
		}
		vars[name] = value
	}

	if len(rest) == 0 || rest[0] == "" {
		fmt.Fprintln(os.Stderr, "jd: just needs a ref (try `jd just <ref> <recipe>`)")
		return 3
	}
	refr := rest[0]
	passthrough := rest[1:]

	justfile, code := renderJustfile(cfg, refr, vars)
	if code != 0 {
		return code
	}

	cmd := exec.Command("just", append([]string{"--justfile", "-"}, passthrough...)...)
	cmd.Stdin = strings.NewReader(justfile)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return exitErr.ExitCode()
		}
		fmt.Fprintf(os.Stderr, "jd: cannot exec `just` (%v) — install it: https://just.systems\n", err)
		return 127
	}
	return 0
}
