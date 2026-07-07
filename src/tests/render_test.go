package tests

import (
	"testing"

	justdown "github.com/yesitsfebreeze/justdown/src"
)

func vars(pairs ...string) justdown.Vars {
	v := justdown.Vars{}
	for i := 0; i+1 < len(pairs); i += 2 {
		v[pairs[i]] = pairs[i+1]
	}
	return v
}

func TestSubstitutesKnownVars(t *testing.T) {
	v := vars("shell", "nu", "cwd", "/tmp")
	if got := justdown.Render("shell=<<shell>> cwd=<<cwd>>", v); got != "shell=nu cwd=/tmp" {
		t.Fatalf("got %q", got)
	}
}

func TestUnknownVarLeftVerbatim(t *testing.T) {
	if got := justdown.Render("<<shell>> <<missing>>", vars("shell", "nu")); got != "nu <<missing>>" {
		t.Fatalf("got %q", got)
	}
}

func TestLiteralDoubleAngleViaQuad(t *testing.T) {
	if got := justdown.Render("<<<<shell>>", vars("shell", "nu")); got != "<<shell>>" {
		t.Fatalf("got %q", got)
	}
}

func TestInjectedValueIsNotRescanned(t *testing.T) {
	v := vars("screen", "danger <<shell>>", "shell", "nu")
	if got := justdown.Render("<<screen>>", v); got != "danger <<shell>>" {
		t.Fatalf("got %q", got)
	}
}

func TestMalformedEscapesPassThrough(t *testing.T) {
	in := "<< a >> <<a b>> <<>> <<a"
	if got := justdown.Render(in, vars("a", "X")); got != in {
		t.Fatalf("got %q", got)
	}
}

func TestLeavesJustInterpolationUntouched(t *testing.T) {
	if got := justdown.Render("open {{t}} <<t>>", vars("t", "file.txt")); got != "open {{t}} file.txt" {
		t.Fatalf("got %q", got)
	}
}

func TestNoEscapesIsIdentity(t *testing.T) {
	in := "plain text {curly} ${sh}"
	if got := justdown.Render(in, justdown.Vars{}); got != in {
		t.Fatalf("got %q", got)
	}
}
