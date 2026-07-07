package cycle

import (
	"fmt"
	"strconv"
	"strings"
	"unicode"
)

type Modifier struct {
	Width   uint32
	Ceiling uint32
	Floor   uint32
	Repeat  *Repeat
}

type RepeatKind int

const (
	RepeatCount RepeatKind = iota
	RepeatEveryMinutes
)

type Repeat struct {
	Kind RepeatKind
	N    uint32
}

type NodeKind int

const (
	KindJob NodeKind = iota
	KindWrap
	KindCycle
)

type Node struct {
	Kind  NodeKind
	Job   string
	Mod   Modifier
	Inner *Node
	Seq   []Node
}

func Job(name string) Node { return Node{Kind: KindJob, Job: name} }

func Wrap(mod Modifier, inner Node) Node {
	return Node{Kind: KindWrap, Mod: mod, Inner: &inner}
}

func Cycle(seq ...Node) Node { return Node{Kind: KindCycle, Seq: seq} }

func (n Node) Equal(o Node) bool {
	if n.Kind != o.Kind {
		return false
	}
	switch n.Kind {
	case KindJob:
		return n.Job == o.Job
	case KindWrap:
		if n.Mod.Width != o.Mod.Width || n.Mod.Ceiling != o.Mod.Ceiling || n.Mod.Floor != o.Mod.Floor {
			return false
		}
		if (n.Mod.Repeat == nil) != (o.Mod.Repeat == nil) {
			return false
		}
		if n.Mod.Repeat != nil && *n.Mod.Repeat != *o.Mod.Repeat {
			return false
		}
		return n.Inner.Equal(*o.Inner)
	case KindCycle:
		if len(n.Seq) != len(o.Seq) {
			return false
		}
		for i := range n.Seq {
			if !n.Seq[i].Equal(o.Seq[i]) {
				return false
			}
		}
		return true
	}
	return false
}

type Chain struct {
	Root Node
}

type ParseErrorKind int

const (
	ErrEmpty ParseErrorKind = iota
	ErrUnbalancedParens
	ErrDuplicateAxis
	ErrBadCount
	ErrTrailingSlash
	ErrEmptyGroup
	ErrUnexpectedChar
)

type ParseError struct {
	Kind ParseErrorKind
	Pos  int
	Axis string
	Char rune
}

func (e *ParseError) Error() string {
	switch e.Kind {
	case ErrEmpty:
		return "empty chain"
	case ErrUnbalancedParens:
		return fmt.Sprintf("unbalanced parentheses at position %d", e.Pos)
	case ErrDuplicateAxis:
		return fmt.Sprintf("duplicate %s at position %d", e.Axis, e.Pos)
	case ErrBadCount:
		return fmt.Sprintf("bad count at position %d", e.Pos)
	case ErrTrailingSlash:
		return fmt.Sprintf("trailing / at position %d", e.Pos)
	case ErrEmptyGroup:
		return fmt.Sprintf("empty group () at position %d", e.Pos)
	case ErrUnexpectedChar:
		return fmt.Sprintf("unexpected character '%c' at position %d", e.Char, e.Pos)
	}
	return "parse error"
}

func IsDirectiveLine(line string) bool {
	trimmed := strings.TrimRight(strings.TrimLeft(line, " \t"), "\n\r")
	if !strings.HasPrefix(trimmed, "<<") {
		return false
	}
	if len(trimmed) < 3 {
		return false // "<<" + space minimum
	}
	return trimmed[2] == ' '
}

type Directive struct {
	Line  int
	Chain *Chain
	Err   *ParseError
}

func Directives(body string) []Directive {
	var out []Directive
	lines := strings.Split(strings.TrimSuffix(body, "\n"), "\n")
	if body == "" {
		lines = nil
	}
	for idx, line := range lines {
		if !IsDirectiveLine(line) {
			continue
		}
		trimmed := strings.TrimRight(strings.TrimLeft(line, " \t"), "\n\r")
		chainSrc := trimmed[3:] // skip "<<" and the mandatory space
		chain, err := ParseChain(chainSrc)
		out = append(out, Directive{Line: idx, Chain: chain, Err: err})
	}
	return out
}

type parser struct {
	input []rune
	pos   int
}

func (p *parser) peek() (rune, bool) {
	if p.pos < len(p.input) {
		return p.input[p.pos], true
	}
	return 0, false
}

func (p *parser) advance() {
	if p.pos < len(p.input) {
		p.pos++
	}
}

func (p *parser) skipWhitespace() {
	for {
		ch, ok := p.peek()
		if !ok || !unicode.IsSpace(ch) {
			return
		}
		p.advance()
	}
}

func (p *parser) parseCount() (uint32, *ParseError) {
	start := p.pos
	ch, ok := p.peek()
	if !ok || ch < '0' || ch > '9' {
		return 0, &ParseError{Kind: ErrBadCount, Pos: p.pos}
	}
	var num strings.Builder
	for {
		ch, ok := p.peek()
		if !ok || ch < '0' || ch > '9' {
			break
		}
		num.WriteRune(ch)
		p.advance()
	}
	val, err := strconv.ParseUint(num.String(), 10, 32)
	if err != nil || val == 0 {
		return 0, &ParseError{Kind: ErrBadCount, Pos: start}
	}
	return uint32(val), nil
}

func (p *parser) parseJob() (string, *ParseError) {
	start := p.pos
	var name strings.Builder
	for {
		ch, ok := p.peek()
		if !ok || !(unicode.IsLetter(ch) || unicode.IsDigit(ch) || ch == '_' || ch == '-' || ch == '/') {
			break
		}
		name.WriteRune(ch)
		p.advance()
	}
	if name.Len() == 0 {
		ch, _ := p.peek()
		return "", &ParseError{Kind: ErrUnexpectedChar, Char: ch, Pos: start}
	}
	return name.String(), nil
}

func (p *parser) parseModifier() (Modifier, *ParseError) {
	var mod Modifier
	foundAny := false
	for {
		ch, ok := p.peek()
		if !ok {
			break
		}
		switch ch {
		case 'x':
			if mod.Width != 0 {
				return mod, &ParseError{Kind: ErrDuplicateAxis, Axis: "x", Pos: p.pos}
			}
			p.advance()
			n, err := p.parseCount()
			if err != nil {
				return mod, err
			}
			mod.Width = n
			foundAny = true
		case '<':
			if mod.Ceiling != 0 || mod.Floor != 0 {
				return mod, &ParseError{Kind: ErrDuplicateAxis, Axis: "count", Pos: p.pos}
			}
			p.advance()
			n, err := p.parseCount()
			if err != nil {
				return mod, err
			}
			mod.Ceiling = n
			foundAny = true
		case '>':
			if mod.Floor != 0 || mod.Ceiling != 0 {
				return mod, &ParseError{Kind: ErrDuplicateAxis, Axis: "count", Pos: p.pos}
			}
			p.advance()
			n, err := p.parseCount()
			if err != nil {
				return mod, err
			}
			if next, ok := p.peek(); ok && next == '<' {
				p.advance()
				m, err := p.parseCount()
				if err != nil {
					return mod, err
				}
				mod.Floor = n
				mod.Ceiling = m
			} else {
				mod.Floor = n
			}
			foundAny = true
		case '*':
			if mod.Repeat != nil {
				return mod, &ParseError{Kind: ErrDuplicateAxis, Axis: "*", Pos: p.pos}
			}
			p.advance()
			n, err := p.parseCount()
			if err != nil {
				return mod, err
			}
			if next, ok := p.peek(); ok && next == 'm' {
				p.advance()
				mod.Repeat = &Repeat{Kind: RepeatEveryMinutes, N: n}
			} else {
				mod.Repeat = &Repeat{Kind: RepeatCount, N: n}
			}
			foundAny = true
		default:
			goto done
		}
	}
done:
	if !foundAny {
		return mod, &ParseError{Kind: ErrEmpty}
	}
	return mod, nil
}

func (p *parser) parseTarget() (Node, *ParseError) {
	if ch, ok := p.peek(); ok && ch == '(' {
		parenPos := p.pos
		p.advance()
		p.skipWhitespace()
		if next, ok := p.peek(); ok && next == ')' {
			return Node{}, &ParseError{Kind: ErrEmptyGroup, Pos: parenPos}
		}
		cyc, err := p.parseCycle()
		if err != nil {
			return Node{}, err
		}
		p.skipWhitespace()
		if next, ok := p.peek(); ok && next == ')' {
			p.advance()
			return cyc, nil
		}
		return Node{}, &ParseError{Kind: ErrUnbalancedParens, Pos: parenPos}
	}
	job, err := p.parseJob()
	if err != nil {
		return Node{}, err
	}
	return Job(job), nil
}

func (p *parser) parseChainInner() (Node, *ParseError) {
	ch, ok := p.peek()
	startsWithMod := ok && (ch == 'x' || ch == '<' || ch == '>' || ch == '*')
	if !startsWithMod {
		return p.parseTarget()
	}
	mod, err := p.parseModifier()
	if err != nil {
		return Node{}, err
	}
	if next, ok := p.peek(); !ok || next != '/' {
		return Node{}, &ParseError{Kind: ErrTrailingSlash, Pos: p.pos}
	}
	p.advance()
	if next, ok := p.peek(); ok && next == '/' {
		return Node{}, &ParseError{Kind: ErrTrailingSlash, Pos: p.pos}
	}
	if _, ok := p.peek(); !ok {
		return Node{}, &ParseError{Kind: ErrTrailingSlash, Pos: p.pos - 1}
	}
	inner, err := p.parseChainInner()
	if err != nil {
		return Node{}, err
	}
	return Wrap(mod, inner), nil
}

func (p *parser) parseCycle() (Node, *ParseError) {
	var chains []Node
	for {
		p.skipWhitespace()
		node, err := p.parseChainInner()
		if err != nil {
			return Node{}, err
		}
		chains = append(chains, node)
		p.skipWhitespace()
		ch, ok := p.peek()
		if !ok || ch == ')' {
			break
		}
		if ch == ',' {
			p.advance()
			continue
		}
		return Node{}, &ParseError{Kind: ErrUnexpectedChar, Char: ch, Pos: p.pos}
	}
	if len(chains) == 0 {
		return Node{}, &ParseError{Kind: ErrEmpty}
	}
	if len(chains) == 1 {
		return chains[0], nil
	}
	return Node{Kind: KindCycle, Seq: chains}, nil
}

func ParseChain(src string) (*Chain, *ParseError) {
	src = strings.TrimSpace(src)
	if src == "" {
		return nil, &ParseError{Kind: ErrEmpty}
	}
	p := &parser{input: []rune(src)}
	p.skipWhitespace()
	root, err := p.parseCycle()
	if err != nil {
		return nil, err
	}
	p.skipWhitespace()
	if p.pos < len(p.input) {
		return nil, &ParseError{Kind: ErrUnexpectedChar, Char: p.input[p.pos], Pos: p.pos}
	}
	return &Chain{Root: root}, nil
}
