package tests

import (
	"testing"

	cycle "github.com/yesitsfebreeze/justdown/src/cycle"
)

func mustParse(t *testing.T, src string) cycle.Node {
	t.Helper()
	c, err := cycle.ParseChain(src)
	if err != nil {
		t.Fatalf("parse %q: %v", src, err)
	}
	return c.Root
}

func assertEqual(t *testing.T, got, want cycle.Node) {
	t.Helper()
	if !got.Equal(want) {
		t.Fatalf("got %+v want %+v", got, want)
	}
}

func TestBareJob(t *testing.T) {
	assertEqual(t, mustParse(t, "plan"), cycle.Job("plan"))
}

func TestBareJobWithSlash(t *testing.T) {
	assertEqual(t, mustParse(t, "jd/improve"), cycle.Job("jd/improve"))
}

func TestWidth(t *testing.T) {
	assertEqual(t, mustParse(t, "x1/plan"), cycle.Wrap(cycle.Modifier{Width: 1}, cycle.Job("plan")))
	assertEqual(t, mustParse(t, "x5/plan"), cycle.Wrap(cycle.Modifier{Width: 5}, cycle.Job("plan")))
}

func TestCounts(t *testing.T) {
	assertEqual(t, mustParse(t, "<3/plan"), cycle.Wrap(cycle.Modifier{Ceiling: 3}, cycle.Job("plan")))
	assertEqual(t, mustParse(t, ">2/plan"), cycle.Wrap(cycle.Modifier{Floor: 2}, cycle.Job("plan")))
	assertEqual(t, mustParse(t, ">2<5/plan"), cycle.Wrap(cycle.Modifier{Floor: 2, Ceiling: 5}, cycle.Job("plan")))
}

func TestRepeat(t *testing.T) {
	assertEqual(t, mustParse(t, "*4/plan"),
		cycle.Wrap(cycle.Modifier{Repeat: &cycle.Repeat{Kind: cycle.RepeatCount, N: 4}}, cycle.Job("plan")))
	assertEqual(t, mustParse(t, "*5m/plan"),
		cycle.Wrap(cycle.Modifier{Repeat: &cycle.Repeat{Kind: cycle.RepeatEveryMinutes, N: 5}}, cycle.Job("plan")))
}

func TestNestingRepeatThenCeiling(t *testing.T) {
	assertEqual(t, mustParse(t, "*5/<5/plan"),
		cycle.Wrap(cycle.Modifier{Repeat: &cycle.Repeat{Kind: cycle.RepeatCount, N: 5}},
			cycle.Wrap(cycle.Modifier{Ceiling: 5}, cycle.Job("plan"))))
}

func TestStackingWidthAndCeiling(t *testing.T) {
	assertEqual(t, mustParse(t, "x5<3/plan"), cycle.Wrap(cycle.Modifier{Width: 5, Ceiling: 3}, cycle.Job("plan")))
}

func TestSequences(t *testing.T) {
	assertEqual(t, mustParse(t, "plan, implement"), cycle.Cycle(cycle.Job("plan"), cycle.Job("implement")))
	assertEqual(t, mustParse(t, "plan, implement, review"),
		cycle.Cycle(cycle.Job("plan"), cycle.Job("implement"), cycle.Job("review")))
	assertEqual(t, mustParse(t, "x5/plan, <3/implement"),
		cycle.Cycle(
			cycle.Wrap(cycle.Modifier{Width: 5}, cycle.Job("plan")),
			cycle.Wrap(cycle.Modifier{Ceiling: 3}, cycle.Job("implement")),
		))
}

func TestGroupedCycle(t *testing.T) {
	assertEqual(t, mustParse(t, "*5m/(plan,review,complaints)"),
		cycle.Wrap(cycle.Modifier{Repeat: &cycle.Repeat{Kind: cycle.RepeatEveryMinutes, N: 5}},
			cycle.Cycle(cycle.Job("plan"), cycle.Job("review"), cycle.Job("complaints"))))
	assertEqual(t, mustParse(t, "*5/(<5/plan,>1/complaints)"),
		cycle.Wrap(cycle.Modifier{Repeat: &cycle.Repeat{Kind: cycle.RepeatCount, N: 5}},
			cycle.Cycle(
				cycle.Wrap(cycle.Modifier{Ceiling: 5}, cycle.Job("plan")),
				cycle.Wrap(cycle.Modifier{Floor: 1}, cycle.Job("complaints")),
			)))
}

func TestDirectiveLineDisambiguation(t *testing.T) {
	if !cycle.IsDirectiveLine("<< plan") || !cycle.IsDirectiveLine("  << plan  ") {
		t.Fatal("directive with space")
	}
	if cycle.IsDirectiveLine("<<plan>>") || cycle.IsDirectiveLine("  <<plan>>  ") {
		t.Fatal("<<plan>> is a var substitution, not a directive")
	}
	if !cycle.IsDirectiveLine("<< plan and more text") {
		t.Fatal("only the ^<<\\s+ pattern matters")
	}
}

func TestErrorCases(t *testing.T) {
	for src, kind := range map[string]cycle.ParseErrorKind{
		"x5x2/plan":       cycle.ErrDuplicateAxis,
		"<3<5/plan":       cycle.ErrDuplicateAxis,
		"x0/plan":         cycle.ErrBadCount,
		"xa/plan":         cycle.ErrBadCount,
		"x5/":             cycle.ErrTrailingSlash,
		"x1/()":           cycle.ErrEmptyGroup,
		"x5/(plan,review": cycle.ErrUnbalancedParens,
		"":                cycle.ErrEmpty,
		"   ":             cycle.ErrEmpty,
		"plan#review":     cycle.ErrUnexpectedChar,
	} {
		_, err := cycle.ParseChain(src)
		if err == nil || err.Kind != kind {
			t.Fatalf("%q: got %v want kind %v", src, err, kind)
		}
	}
	if _, err := cycle.ParseChain(">3<5/plan"); err != nil {
		t.Fatalf("range: %v", err)
	}
}

func TestDirectivesExtraction(t *testing.T) {
	dirs := cycle.Directives("Some text\n<< plan\nMore text")
	if len(dirs) != 1 || dirs[0].Line != 1 || dirs[0].Err != nil {
		t.Fatalf("%+v", dirs)
	}
	dirs = cycle.Directives("<< plan\nSome text\n<< implement\nMore")
	if len(dirs) != 2 || dirs[0].Line != 0 || dirs[1].Line != 2 {
		t.Fatalf("%+v", dirs)
	}
	dirs = cycle.Directives("normal line\n<< plan\n<<var>> substitution\n<< implement")
	if len(dirs) != 2 {
		t.Fatalf("only the two with space: %+v", dirs)
	}
	dirs = cycle.Directives("<< x5x2/plan")
	if len(dirs) != 1 || dirs[0].Err == nil {
		t.Fatalf("parse error captured: %+v", dirs)
	}
	dirs = cycle.Directives("<< ")
	if len(dirs) != 1 || dirs[0].Err == nil {
		t.Fatalf("empty directive is an error: %+v", dirs)
	}
}

func TestEquivalencePrefixDirective(t *testing.T) {
	for _, src := range []string{"plan", "x5/plan, implement"} {
		p, err := cycle.ParseChain(src)
		if err != nil {
			t.Fatal(err)
		}
		dirs := cycle.Directives("<< " + src)
		if len(dirs) != 1 || dirs[0].Err != nil {
			t.Fatalf("%+v", dirs)
		}
		if !p.Root.Equal(dirs[0].Chain.Root) {
			t.Fatalf("prefix and directive parse must agree for %q", src)
		}
	}
}
