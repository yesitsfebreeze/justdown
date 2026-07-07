package tests

import (
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

var jdBin string

func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "jd-bin")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	defer os.RemoveAll(dir)
	jdBin = filepath.Join(dir, "jd")
	if out, err := exec.Command("go", "build", "-o", jdBin, "../cmd/jd").CombinedOutput(); err != nil {
		fmt.Fprintf(os.Stderr, "build jd: %v\n%s", err, out)
		os.Exit(1)
	}
	os.Exit(m.Run())
}

func writeFile(t *testing.T, path, body string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(body), 0o644); err != nil {
		t.Fatal(err)
	}
}

func jdFile(name string) string {
	return fmt.Sprintf("---\nname: %s\nkind: tool\ndescription: the %s tool\n---\nbody of %s\n", name, name, name)
}

func buildFixture(t *testing.T) string {
	root := t.TempDir()
	lib := func(home, rel string) string {
		return filepath.Join(root, home, "library", rel)
	}
	writeFile(t, lib(".jd", "core/root-tool.jd"), jdFile("root_tool"))
	writeFile(t, lib(".jd", "shared/dup.jd"), jdFile("from_root"))
	writeFile(t, lib("packages/a/.jd", "pkg/a-tool.jd"), jdFile("a_tool"))
	writeFile(t, lib("packages/a/.jd", "shared/dup.jd"), jdFile("from_a"))
	writeFile(t, lib(".voit/.jd", "voit/v-tool.jd"), jdFile("v_tool"))
	writeFile(t, lib("node_modules/dep/.jd", "junk/nope.jd"), jdFile("nope"))
	return root
}

func runJD(t *testing.T, root string, nested bool, args ...string) (int, string, string) {
	t.Helper()
	cmd := exec.Command(jdBin, args...)
	nestedVal := "0"
	if nested {
		nestedVal = "1"
	}
	cmd.Env = append(os.Environ(),
		"JUSTDOWN_ROOT="+filepath.Join(root, ".jd"),
		"HOME="+filepath.Join(root, "xhome"),
		"XDG_CACHE_HOME="+filepath.Join(root, "xcache"),
		"JUSTDOWN_NESTED="+nestedVal,
		"JUSTDOWN_REPOS=https://example.invalid/none/none",
	)
	var stdout, stderr strings.Builder
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	err := cmd.Run()
	code := 0
	if exitErr, ok := err.(*exec.ExitError); ok {
		code = exitErr.ExitCode()
	} else if err != nil {
		t.Fatal(err)
	}
	return code, stdout.String(), stderr.String()
}

func TestRecursiveBuildAndRootUnionResolveAllHomes(t *testing.T) {
	root := buildFixture(t)

	code, _, stderr := runJD(t, root, true, "build")
	if code != 0 {
		t.Fatalf("build failed: %s", stderr)
	}
	if _, err := os.Stat(filepath.Join(root, ".jd/remote-graph.db")); err != nil {
		t.Fatal("merged remote-graph.db not built")
	}
	if _, err := os.Stat(filepath.Join(root, "node_modules/dep/.jd/remote-graph.db")); err == nil {
		t.Fatal("pruned home must not be built")
	}

	for _, key := range []string{"core/root-tool", "pkg/a-tool", "voit/v-tool"} {
		code, out, stderr := runJD(t, root, true, "get", key, "--frontmatter")
		if code != 0 {
			t.Fatalf("get %s failed: %s\n%s", key, stderr, out)
		}
	}

	code, out, _ := runJD(t, root, true, "ls")
	if code != 0 {
		t.Fatal("ls failed")
	}
	for _, cat := range []string{"core", "pkg", "voit", "shared"} {
		if !strings.Contains(out, cat) {
			t.Fatalf("ls missing category %s:\n%s", cat, out)
		}
	}
	if strings.Contains(out, "junk") {
		t.Fatalf("pruned home leaked into ls:\n%s", out)
	}

	code, out, stderr = runJD(t, root, true, "get", "shared/dup", "--human")
	if code != 0 {
		t.Fatalf("get shared/dup failed: %s", stderr)
	}
	if !strings.Contains(out, "from_a") {
		t.Fatalf("deeper home must win the key collision, got:\n%s", out)
	}
}

func TestNestedDisabledIsLegacySingleHome(t *testing.T) {
	root := buildFixture(t)
	if code, _, stderr := runJD(t, root, false, "build"); code != 0 {
		t.Fatalf("root build failed: %s", stderr)
	}
	if code, _, _ := runJD(t, root, false, "get", "core/root-tool", "--frontmatter"); code != 0 {
		t.Fatal("root key must resolve in legacy mode")
	}
	if code, _, _ := runJD(t, root, false, "get", "pkg/a-tool", "--frontmatter"); code != 2 {
		t.Fatal("nested key must NOT resolve when nesting is disabled")
	}
	code, out, _ := runJD(t, root, false, "get", "shared/dup", "--human")
	if code != 0 || !strings.Contains(out, "from_root") {
		t.Fatalf("legacy union must use the root's shared/dup, got:\n%s", out)
	}
}

func TestNoPanicOnEarlyPipeClose(t *testing.T) {
	root := buildFixture(t)
	if code, _, stderr := runJD(t, root, true, "build"); code != 0 {
		t.Fatalf("build failed: %s", stderr)
	}
	for _, args := range [][]string{
		{"ls"},
		{"--json", "ls"},
		{"--json", "search", "the", "tool", "100"},
	} {
		cmd := exec.Command(jdBin, args...)
		cmd.Env = append(os.Environ(),
			"JUSTDOWN_ROOT="+filepath.Join(root, ".jd"),
			"HOME="+filepath.Join(root, "xhome"),
			"XDG_CACHE_HOME="+filepath.Join(root, "xcache"),
			"JUSTDOWN_REPOS=https://example.invalid/none/none",
		)
		stdout, err := cmd.StdoutPipe()
		if err != nil {
			t.Fatal(err)
		}
		var stderr strings.Builder
		cmd.Stderr = &stderr
		if err := cmd.Start(); err != nil {
			t.Fatal(err)
		}
		buf := make([]byte, 1)
		for {
			n, err := stdout.Read(buf)
			if n == 0 || err == io.EOF || err != nil || buf[0] == '\n' {
				break
			}
		}
		stdout.Close()
		_ = cmd.Wait()
		errText := stderr.String()
		for _, bad := range []string{"broken pipe", "panic", "goroutine"} {
			if strings.Contains(strings.ToLower(errText), bad) {
				t.Fatalf("jd panicked on a closed pipe (args %v):\n%s", args, errText)
			}
		}
	}
}
