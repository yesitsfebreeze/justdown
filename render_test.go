package justdown

import "testing"

func vars(pairs ...string) Vars {
	v := Vars{}
	for i := 0; i+1 < len(pairs); i += 2 {
		v[pairs[i]] = pairs[i+1]
	}
	return v
}

func TestSubstitutesKnownVars(t *testing.T) {
	v := vars("shell", "nu", "cwd", "/tmp")
	if got := Render("shell=<<shell>> cwd=<<cwd>>", v); got != "shell=nu cwd=/tmp" {
		t.Fatalf("got %q", got)
	}
}

func TestUnknownVarLeftVerbatim(t *testing.T) {
	if got := Render("<<shell>> <<missing>>", vars("shell", "nu")); got != "nu <<missing>>" {
		t.Fatalf("got %q", got)
	}
}

func TestLiteralDoubleAngleViaQuad(t *testing.T) {
	if got := Render("<<<<shell>>", vars("shell", "nu")); got != "<<shell>>" {
		t.Fatalf("got %q", got)
	}
}

func TestInjectedValueIsNotRescanned(t *testing.T) {
	v := vars("screen", "danger <<shell>>", "shell", "nu")
	if got := Render("<<screen>>", v); got != "danger <<shell>>" {
		t.Fatalf("got %q", got)
	}
}

func TestMalformedEscapesPassThrough(t *testing.T) {
	in := "<< a >> <<a b>> <<>> <<a"
	if got := Render(in, vars("a", "X")); got != in {
		t.Fatalf("got %q", got)
	}
}

func TestLeavesJustInterpolationUntouched(t *testing.T) {
	if got := Render("open {{t}} <<t>>", vars("t", "file.txt")); got != "open {{t}} file.txt" {
		t.Fatalf("got %q", got)
	}
}

func TestNoEscapesIsIdentity(t *testing.T) {
	in := "plain text {curly} ${sh}"
	if got := Render(in, Vars{}); got != in {
		t.Fatalf("got %q", got)
	}
}
