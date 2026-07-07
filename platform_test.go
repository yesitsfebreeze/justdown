package justdown

import (
	"reflect"
	"strings"
	"testing"
)

func sel(src, plat string) string {
	return strings.Join(Platsel(Lines(src), plat), "\n")
}

func TestPicksOneVariantPerHostAndStripsAttrs(t *testing.T) {
	src := "[unix]\nopen t:\n  xdg-open {{t}}\n[macos]\nopen t:\n  open {{t}}\n[windows]\nopen t:\n  start {{t}}\n[wsl]\nopen t:\n  wslview {{t}}"
	for plat, want := range map[string]string{
		"unix":    "open t:\n  xdg-open {{t}}",
		"macos":   "open t:\n  open {{t}}",
		"windows": "open t:\n  start {{t}}",
		"wsl":     "open t:\n  wslview {{t}}",
	} {
		if got := sel(src, plat); got != want {
			t.Fatalf("%s: %q", plat, got)
		}
	}
}

func TestCommaListAndDarwinAlias(t *testing.T) {
	src := "[unix, wsl]\nr:\n  a\n[macos]\nr:\n  b"
	if sel(src, "unix") != "r:\n  a" || sel(src, "wsl") != "r:\n  a" || sel(src, "macos") != "r:\n  b" {
		t.Fatal("comma list")
	}
	darwin := "[darwin]\nr:\n  mac"
	if sel(darwin, "macos") != "r:\n  mac" || sel(darwin, "unix") != "" {
		t.Fatal("darwin alias")
	}
}

func TestUntaggedAndNonplatformAttrsPassThrough(t *testing.T) {
	src := "# desc\nr:\n  body\n[unix]\nr2:\n  ux"
	if got := sel(src, "macos"); got != "# desc\nr:\n  body" {
		t.Fatalf("got %q", got)
	}
	if ParsePlatformAttr("[private]") != nil || ParsePlatformAttr("[confirm: \"sure?\"]") != nil {
		t.Fatal("non-platform attrs must pass")
	}
	keep := "[private]\nr:\n  body"
	if got := sel(keep, "unix"); got != keep {
		t.Fatalf("got %q", got)
	}
}

func TestParsesTagLists(t *testing.T) {
	if !reflect.DeepEqual(ParsePlatformAttr("[unix]"), []string{"unix"}) {
		t.Fatal("[unix]")
	}
	if !reflect.DeepEqual(ParsePlatformAttr("[ unix , wsl ]"), []string{"unix", "wsl"}) {
		t.Fatal("[ unix , wsl ]")
	}
	if ParsePlatformAttr("not an attr") != nil || ParsePlatformAttr("[unix, bogus]") != nil {
		t.Fatal("invalid attrs")
	}
}

func TestSelectForHostRespectsJDPlatformOverride(t *testing.T) {
	t.Setenv("JD_PLATFORM", "macos")
	src := "[unix]\nopen t:\n  xdg-open {{t}}\n[macos]\nopen t:\n  open {{t}}"
	if got := SelectForHost(src); got != "open t:\n  open {{t}}" {
		t.Fatalf("got %q", got)
	}
}
