package cycle

import "testing"

func mustParse(t *testing.T, src string) Node {
	t.Helper()
	c, err := ParseChain(src)
	if err != nil {
		t.Fatalf("parse %q: %v", src, err)
	}
	return c.Root
}

func assertEqual(t *testing.T, got, want Node) {
	t.Helper()
	if !got.Equal(want) {
		t.Fatalf("got %+v want %+v", got, want)
	}
}

func TestBareJob(t *testing.T) {
	assertEqual(t, mustParse(t, "plan"), Job("plan"))
}

func TestBareJobWithSlash(t *testing.T) {
	assertEqual(t, mustParse(t, "jd/improve"), Job("jd/improve"))
}

func TestWidth(t *testing.T) {
	assertEqual(t, mustParse(t, "x1/plan"), Wrap(Modifier{Width: 1}, Job("plan")))
	assertEqual(t, mustParse(t, "x5/plan"), Wrap(Modifier{Width: 5}, Job("plan")))
}

func TestCounts(t *testing.T) {
	assertEqual(t, mustParse(t, "<3/plan"), Wrap(Modifier{Ceiling: 3}, Job("plan")))
	assertEqual(t, mustParse(t, ">2/plan"), Wrap(Modifier{Floor: 2}, Job("plan")))
	assertEqual(t, mustParse(t, ">2<5/plan"), Wrap(Modifier{Floor: 2, Ceiling: 5}, Job("plan")))
}

func TestRepeat(t *testing.T) {
	assertEqual(t, mustParse(t, "*4/plan"),
		Wrap(Modifier{Repeat: &Repeat{Kind: RepeatCount, N: 4}}, Job("plan")))
	assertEqual(t, mustParse(t, "*5m/plan"),
		Wrap(Modifier{Repeat: &Repeat{Kind: RepeatEveryMinutes, N: 5}}, Job("plan")))
}

func TestNestingRepeatThenCeiling(t *testing.T) {
	assertEqual(t, mustParse(t, "*5/<5/plan"),
		Wrap(Modifier{Repeat: &Repeat{Kind: RepeatCount, N: 5}},
			Wrap(Modifier{Ceiling: 5}, Job("plan"))))
}

func TestStackingWidthAndCeiling(t *testing.T) {
	assertEqual(t, mustParse(t, "x5<3/plan"), Wrap(Modifier{Width: 5, Ceiling: 3}, Job("plan")))
}

func TestSequences(t *testing.T) {
	assertEqual(t, mustParse(t, "plan, implement"), Cycle(Job("plan"), Job("implement")))
	assertEqual(t, mustParse(t, "plan, implement, review"),
		Cycle(Job("plan"), Job("implement"), Job("review")))
	assertEqual(t, mustParse(t, "x5/plan, <3/implement"),
		Cycle(
			Wrap(Modifier{Width: 5}, Job("plan")),
			Wrap(Modifier{Ceiling: 3}, Job("implement")),
		))
}

func TestGroupedCycle(t *testing.T) {
	assertEqual(t, mustParse(t, "*5m/(plan,review,complaints)"),
		Wrap(Modifier{Repeat: &Repeat{Kind: RepeatEveryMinutes, N: 5}},
			Cycle(Job("plan"), Job("review"), Job("complaints"))))
	assertEqual(t, mustParse(t, "*5/(<5/plan,>1/complaints)"),
		Wrap(Modifier{Repeat: &Repeat{Kind: RepeatCount, N: 5}},
			Cycle(
				Wrap(Modifier{Ceiling: 5}, Job("plan")),
				Wrap(Modifier{Floor: 1}, Job("complaints")),
			)))
}

func TestDirectiveLineDisambiguation(t *testing.T) {
	if !IsDirectiveLine("<< plan") || !IsDirectiveLine("  << plan  ") {
		t.Fatal("directive with space")
	}
	if IsDirectiveLine("<<plan>>") || IsDirectiveLine("  <<plan>>  ") {
		t.Fatal("<<plan>> is a var substitution, not a directive")
	}
	if !IsDirectiveLine("<< plan and more text") {
		t.Fatal("only the ^<<\\s+ pattern matters")
	}
}

func TestErrorCases(t *testing.T) {
	for src, kind := range map[string]ParseErrorKind{
		"x5x2/plan":       ErrDuplicateAxis,
		"<3<5/plan":       ErrDuplicateAxis,
		"x0/plan":         ErrBadCount,
		"xa/plan":         ErrBadCount,
		"x5/":             ErrTrailingSlash,
		"x1/()":           ErrEmptyGroup,
		"x5/(plan,review": ErrUnbalancedParens,
		"":                ErrEmpty,
		"   ":             ErrEmpty,
		"plan#review":     ErrUnexpectedChar,
	} {
		_, err := ParseChain(src)
		if err == nil || err.Kind != kind {
			t.Fatalf("%q: got %v want kind %v", src, err, kind)
		}
	}
	if _, err := ParseChain(">3<5/plan"); err != nil {
		t.Fatalf("range: %v", err)
	}
}

func TestDirectivesExtraction(t *testing.T) {
	dirs := Directives("Some text\n<< plan\nMore text")
	if len(dirs) != 1 || dirs[0].Line != 1 || dirs[0].Err != nil {
		t.Fatalf("%+v", dirs)
	}
	dirs = Directives("<< plan\nSome text\n<< implement\nMore")
	if len(dirs) != 2 || dirs[0].Line != 0 || dirs[1].Line != 2 {
		t.Fatalf("%+v", dirs)
	}
	dirs = Directives("normal line\n<< plan\n<<var>> substitution\n<< implement")
	if len(dirs) != 2 {
		t.Fatalf("only the two with space: %+v", dirs)
	}
	dirs = Directives("<< x5x2/plan")
	if len(dirs) != 1 || dirs[0].Err == nil {
		t.Fatalf("parse error captured: %+v", dirs)
	}
	dirs = Directives("<< ")
	if len(dirs) != 1 || dirs[0].Err == nil {
		t.Fatalf("empty directive is an error: %+v", dirs)
	}
}

func TestEquivalencePrefixDirective(t *testing.T) {
	for _, src := range []string{"plan", "x5/plan, implement"} {
		p, err := ParseChain(src)
		if err != nil {
			t.Fatal(err)
		}
		dirs := Directives("<< " + src)
		if len(dirs) != 1 || dirs[0].Err != nil {
			t.Fatalf("%+v", dirs)
		}
		if !p.Root.Equal(dirs[0].Chain.Root) {
			t.Fatalf("prefix and directive parse must agree for %q", src)
		}
	}
}
