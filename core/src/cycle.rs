//! Composable iteration/cycle engine for `.jd` job directives.
//!
//! A `.jd` line starting with `<<` (followed by mandatory space) marks a job
//! directive. The `cycle` module provides:
//! - A pure **grammar parser** (`parse_chain`) producing an AST that represents
//!   composable iteration patterns: jobs, panel widths, count bounds, repeats.
//! - **AST types** (`Node`, `Chain`, `Modifier`, `Repeat`) for inspecting cycles
//!   before execution.
//! - **Line classification** (`is_directive_line`, `directives`) to extract and
//!   parse `<<` directives from `.jd` body text.
//!
//! No execution, no I/O, no model calls — purely syntactic.

use std::fmt;

// ============================================================================
// AST Types
// ============================================================================

/// A `Modifier` wraps zero or more orthogonal control axes applied to a Node.
/// At most one of each: `width`, `floor`, `ceiling`, `repeat`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Modifier {
    /// `x N` — run N parallel jobs + consolidation.
    pub width: Option<u32>,
    /// `< N` — stop after N successful runs (ceiling/early exit).
    pub ceiling: Option<u32>,
    /// `> N` — force at least N runs (floor).
    pub floor: Option<u32>,
    /// `* N` (Count) or `* N m` (EveryMinutes).
    pub repeat: Option<Repeat>,
}

/// Repeat axis: iterate N times or every N minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repeat {
    Count(u32),
    EveryMinutes(u32),
}

/// A job or group of jobs. `/` nests outside-in (leftmost = outermost).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A single job name (e.g., "plan", "jd/improve").
    Job(String),
    /// A modifier wrapping a nested Node (e.g., `x5/plan` → `Wrap(Modifier{width:5}, Job("plan"))`).
    Wrap(Modifier, Box<Node>),
    /// A comma-separated sequence of Nodes forming a loop body.
    Cycle(Vec<Node>),
}

/// A parsed chain: the root of the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub root: Node,
}

/// Errors from the parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Empty input or no valid content.
    Empty,
    /// Unmatched `(` or `)`.
    UnbalancedParens { pos: usize },
    /// Multiple `x` or multiple count-ops in one modifier.
    DuplicateAxis { axis: &'static str, pos: usize },
    /// Invalid count (0, negative, or non-numeric).
    BadCount { pos: usize },
    /// Modifier followed by `/` but no target (e.g., `x5/` trailing).
    TrailingSlash { pos: usize },
    /// Empty group `()`.
    EmptyGroup { pos: usize },
    /// Unexpected character.
    UnexpectedChar { ch: char, pos: usize },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty chain"),
            ParseError::UnbalancedParens { pos } => {
                write!(f, "unbalanced parentheses at position {}", pos)
            }
            ParseError::DuplicateAxis { axis, pos } => {
                write!(f, "duplicate {} at position {}", axis, pos)
            }
            ParseError::BadCount { pos } => {
                write!(f, "bad count at position {}", pos)
            }
            ParseError::TrailingSlash { pos } => {
                write!(f, "trailing / at position {}", pos)
            }
            ParseError::EmptyGroup { pos } => {
                write!(f, "empty group () at position {}", pos)
            }
            ParseError::UnexpectedChar { ch, pos } => {
                write!(f, "unexpected character '{}' at position {}", ch, pos)
            }
        }
    }
}

// ============================================================================
// Line Classification & Extraction
// ============================================================================

/// Determine if a line is a `<<` job directive.
/// A directive line is `^<<\s+<chain>?$` — `<<` + mandatory single space (+ optional chain).
/// This disambiguates from `<<name>>` var substitution (which has no internal space).
pub fn is_directive_line(line: &str) -> bool {
    // Only trim leading whitespace and newlines, preserve trailing spaces
    let trimmed = line
        .trim_start()
        .trim_end_matches('\n')
        .trim_end_matches('\r');
    if !trimmed.starts_with("<<") {
        return false;
    }
    if trimmed.len() < 3 {
        return false; // "<<" + space minimum
    }
    // Must have a space after <<
    trimmed[2..].starts_with(' ')
}

/// Extract all `<<` directives from a body, returning (line_number, parse_result).
/// Line numbers are 0-indexed. Parse errors are returned, not panicked.
pub fn directives(body: &str) -> Vec<(usize, Result<Chain, ParseError>)> {
    body.lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            if is_directive_line(line) {
                let trimmed = line
                    .trim_start()
                    .trim_end_matches('\n')
                    .trim_end_matches('\r');
                // Skip "<<" and the mandatory space
                let chain_src = &trimmed[3..];
                Some((idx, parse_chain(chain_src)))
            } else {
                None
            }
        })
        .collect()
}

// ============================================================================
// Parser
// ============================================================================

struct Parser {
    input: Vec<char>,
    pos: usize,
}

impl Parser {
    fn new(s: &str) -> Self {
        Parser {
            input: s.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            None
        }
    }

    fn advance(&mut self) -> Option<char> {
        if self.pos < self.input.len() {
            let ch = self.input[self.pos];
            self.pos += 1;
            Some(ch)
        } else {
            None
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Parse a count: `[1-9][0-9]*`. Returns error if next char is not a digit.
    fn parse_count(&mut self) -> Result<u32, ParseError> {
        let start = self.pos;
        if let Some(ch) = self.peek() {
            if !ch.is_ascii_digit() {
                return Err(ParseError::BadCount { pos: self.pos });
            }
        } else {
            return Err(ParseError::BadCount { pos: self.pos });
        }

        let mut num_str = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                num_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let val: u32 = num_str
            .parse()
            .map_err(|_| ParseError::BadCount { pos: start })?;
        if val == 0 {
            return Err(ParseError::BadCount { pos: start });
        }
        Ok(val)
    }

    /// Parse a job name: `[A-Za-z0-9_/-]+`. A job name contains alphanumerics,
    /// underscores, and forward slashes (e.g., "plan", "jd/improve").
    fn parse_job(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        let mut name = String::new();

        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '/' {
                name.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if name.is_empty() {
            Err(ParseError::UnexpectedChar {
                ch: self.peek().unwrap_or('\0'),
                pos: start,
            })
        } else {
            Ok(name)
        }
    }

    /// Parse a modifier: `[width] [count-op] [repeat]`.
    /// Enforces: at most one `x`, at most one count-op, and no duplicate axes.
    fn parse_modifier(&mut self) -> Result<Modifier, ParseError> {
        let mut mod_obj = Modifier::default();
        let mut found_any = false;

        loop {
            match self.peek() {
                Some('x') => {
                    if mod_obj.width.is_some() {
                        return Err(ParseError::DuplicateAxis {
                            axis: "x",
                            pos: self.pos,
                        });
                    }
                    self.advance();
                    let n = self.parse_count()?;
                    mod_obj.width = Some(n);
                    found_any = true;
                }
                Some('<') => {
                    if mod_obj.ceiling.is_some() || mod_obj.floor.is_some() {
                        return Err(ParseError::DuplicateAxis {
                            axis: "count",
                            pos: self.pos,
                        });
                    }
                    self.advance();
                    let n = self.parse_count()?;
                    mod_obj.ceiling = Some(n);
                    found_any = true;
                }
                Some('>') => {
                    if mod_obj.floor.is_some() || mod_obj.ceiling.is_some() {
                        return Err(ParseError::DuplicateAxis {
                            axis: "count",
                            pos: self.pos,
                        });
                    }
                    self.advance();
                    let n = self.parse_count()?;
                    // Check for range: >N<M
                    if let Some('<') = self.peek() {
                        self.advance();
                        let m = self.parse_count()?;
                        mod_obj.floor = Some(n);
                        mod_obj.ceiling = Some(m);
                    } else {
                        mod_obj.floor = Some(n);
                    }
                    found_any = true;
                }
                Some('*') => {
                    if mod_obj.repeat.is_some() {
                        return Err(ParseError::DuplicateAxis {
                            axis: "*",
                            pos: self.pos,
                        });
                    }
                    self.advance();
                    let n = self.parse_count()?;
                    // Check for temporal: *Nm
                    if let Some('m') = self.peek() {
                        self.advance();
                        mod_obj.repeat = Some(Repeat::EveryMinutes(n));
                    } else {
                        mod_obj.repeat = Some(Repeat::Count(n));
                    }
                    found_any = true;
                }
                _ => break,
            }
        }

        if !found_any {
            // No modifier was parsed
            return Err(ParseError::Empty);
        }

        Ok(mod_obj)
    }

    /// Parse a target: a job or a grouped cycle.
    fn parse_target(&mut self) -> Result<Node, ParseError> {
        match self.peek() {
            Some('(') => {
                let paren_pos = self.pos;
                self.advance();
                self.skip_whitespace();

                if let Some(')') = self.peek() {
                    return Err(ParseError::EmptyGroup { pos: paren_pos });
                }

                let cycle = self.parse_cycle()?;
                self.skip_whitespace();

                if let Some(')') = self.peek() {
                    self.advance();
                    Ok(cycle)
                } else {
                    Err(ParseError::UnbalancedParens { pos: paren_pos })
                }
            }
            _ => {
                let job = self.parse_job()?;
                Ok(Node::Job(job))
            }
        }
    }

    /// Parse a chain: a sequence of `/`-separated modifier/target pairs.
    /// `/` nests outside-in: `x5/<3/plan` = `Wrap(x5, Wrap(<3, Job(plan)))`.
    fn parse_chain_inner(&mut self) -> Result<Node, ParseError> {
        // Look ahead to detect if we start with a modifier or a target.
        // A modifier is triggered by: x, <, >, *
        let starts_with_mod = matches!(self.peek(), Some('x' | '<' | '>' | '*'));

        if starts_with_mod {
            // Parse as modifier / target
            let mod_obj = self.parse_modifier()?;
            if let Some('/') = self.peek() {
                self.advance();
                if let Some('/') = self.peek() {
                    return Err(ParseError::TrailingSlash { pos: self.pos });
                }
                // Check if there's anything after the /
                if self.peek().is_none() {
                    return Err(ParseError::TrailingSlash { pos: self.pos - 1 });
                }
                let inner = self.parse_chain_inner()?;
                Ok(Node::Wrap(mod_obj, Box::new(inner)))
            } else {
                // Modifier not followed by `/` — this is an error.
                Err(ParseError::TrailingSlash { pos: self.pos })
            }
        } else {
            // No modifier; parse as target.
            self.parse_target()
        }
    }

    /// Parse a cycle: comma-separated chains (grouped by `()`).
    fn parse_cycle(&mut self) -> Result<Node, ParseError> {
        let mut chains = vec![];

        loop {
            self.skip_whitespace();
            let node = self.parse_chain_inner()?;
            chains.push(node);

            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    self.advance();
                }
                Some(')') | None => break,
                _ => {
                    // Trailing garbage
                    if let Some(ch) = self.peek() {
                        return Err(ParseError::UnexpectedChar { ch, pos: self.pos });
                    }
                    break;
                }
            }
        }

        if chains.is_empty() {
            Err(ParseError::Empty)
        } else if chains.len() == 1 {
            Ok(chains.into_iter().next().unwrap())
        } else {
            Ok(Node::Cycle(chains))
        }
    }
}

/// Parse a chain from a string.
/// Returns `Chain` with the root AST, or a `ParseError` if invalid.
///
/// Grammar (BNF):
/// ```text
/// cycle      = chain ( "," chain )*
/// chain      = mod "/" chain | target
/// target     = job | "(" cycle ")"
/// job        = [A-Za-z0-9_/-]+
/// mod        = width? count? repeat?
/// width      = "x" N
/// count      = "<" N | ">" N | ">" N "<" M
/// repeat     = "*" N | "*" N "m"
/// N, M       = [1-9][0-9]*
/// ```
pub fn parse_chain(src: &str) -> Result<Chain, ParseError> {
    let src = src.trim();
    if src.is_empty() {
        return Err(ParseError::Empty);
    }

    let mut parser = Parser::new(src);
    parser.skip_whitespace();

    let root = parser.parse_cycle()?;

    parser.skip_whitespace();
    if parser.pos < parser.input.len() {
        return Err(ParseError::UnexpectedChar {
            ch: parser.input[parser.pos],
            pos: parser.pos,
        });
    }

    Ok(Chain { root })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Atoms ==========

    #[test]
    fn test_bare_job() {
        let res = parse_chain("plan").unwrap();
        assert_eq!(res.root, Node::Job("plan".to_string()));
    }

    #[test]
    fn test_bare_job_with_slash() {
        let res = parse_chain("jd/improve").unwrap();
        assert_eq!(res.root, Node::Job("jd/improve".to_string()));
    }

    #[test]
    fn test_x1_equivalent_to_bare() {
        // x1/job is equivalent to job (panel of 1 = single run)
        let res = parse_chain("x1/plan").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: Some(1),
                    ceiling: None,
                    floor: None,
                    repeat: None
                },
                Box::new(Node::Job("plan".to_string()))
            )
        );
    }

    #[test]
    fn test_width_x5() {
        let res = parse_chain("x5/plan").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: Some(5),
                    ceiling: None,
                    floor: None,
                    repeat: None
                },
                Box::new(Node::Job("plan".to_string()))
            )
        );
    }

    #[test]
    fn test_ceiling_less_than_3() {
        let res = parse_chain("<3/plan").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: None,
                    ceiling: Some(3),
                    floor: None,
                    repeat: None
                },
                Box::new(Node::Job("plan".to_string()))
            )
        );
    }

    #[test]
    fn test_floor_greater_than_2() {
        let res = parse_chain(">2/plan").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: None,
                    ceiling: None,
                    floor: Some(2),
                    repeat: None
                },
                Box::new(Node::Job("plan".to_string()))
            )
        );
    }

    #[test]
    fn test_range_floor_and_ceiling() {
        let res = parse_chain(">2<5/plan").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: None,
                    ceiling: Some(5),
                    floor: Some(2),
                    repeat: None
                },
                Box::new(Node::Job("plan".to_string()))
            )
        );
    }

    #[test]
    fn test_repeat_count_4() {
        let res = parse_chain("*4/plan").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: None,
                    ceiling: None,
                    floor: None,
                    repeat: Some(Repeat::Count(4))
                },
                Box::new(Node::Job("plan".to_string()))
            )
        );
    }

    #[test]
    fn test_repeat_temporal_5m() {
        let res = parse_chain("*5m/plan").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: None,
                    ceiling: None,
                    floor: None,
                    repeat: Some(Repeat::EveryMinutes(5))
                },
                Box::new(Node::Job("plan".to_string()))
            )
        );
    }

    // ========== Nesting ==========

    #[test]
    fn test_nesting_repeat_then_ceiling() {
        // *5/<5/plan = Repeat(5, Ceiling(5, Job))
        let res = parse_chain("*5/<5/plan").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: None,
                    ceiling: None,
                    floor: None,
                    repeat: Some(Repeat::Count(5))
                },
                Box::new(Node::Wrap(
                    Modifier {
                        width: None,
                        ceiling: Some(5),
                        floor: None,
                        repeat: None
                    },
                    Box::new(Node::Job("plan".to_string()))
                ))
            )
        );
    }

    // ========== Stacking in one modifier ==========

    #[test]
    fn test_stacking_width_and_ceiling() {
        // x5<3/plan = Wrap(Modifier{width:5, ceiling:3}, Job)
        let res = parse_chain("x5<3/plan").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: Some(5),
                    ceiling: Some(3),
                    floor: None,
                    repeat: None
                },
                Box::new(Node::Job("plan".to_string()))
            )
        );
    }

    #[test]
    fn test_stacking_floor_and_ceiling_in_modifier() {
        // >2<5/plan = Wrap(Modifier{floor:2, ceiling:5}, Job)
        let res = parse_chain(">2<5/plan").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: None,
                    ceiling: Some(5),
                    floor: Some(2),
                    repeat: None
                },
                Box::new(Node::Job("plan".to_string()))
            )
        );
    }

    // ========== Sequencing ==========

    #[test]
    fn test_sequence_two_jobs() {
        let res = parse_chain("plan, implement").unwrap();
        assert_eq!(
            res.root,
            Node::Cycle(vec![
                Node::Job("plan".to_string()),
                Node::Job("implement".to_string())
            ])
        );
    }

    #[test]
    fn test_sequence_three_jobs() {
        let res = parse_chain("plan, implement, review").unwrap();
        assert_eq!(
            res.root,
            Node::Cycle(vec![
                Node::Job("plan".to_string()),
                Node::Job("implement".to_string()),
                Node::Job("review".to_string())
            ])
        );
    }

    #[test]
    fn test_sequence_with_modifiers() {
        let res = parse_chain("x5/plan, <3/implement").unwrap();
        assert_eq!(
            res.root,
            Node::Cycle(vec![
                Node::Wrap(
                    Modifier {
                        width: Some(5),
                        ceiling: None,
                        floor: None,
                        repeat: None
                    },
                    Box::new(Node::Job("plan".to_string()))
                ),
                Node::Wrap(
                    Modifier {
                        width: None,
                        ceiling: Some(3),
                        floor: None,
                        repeat: None
                    },
                    Box::new(Node::Job("implement".to_string()))
                )
            ])
        );
    }

    // ========== Grouping ==========

    #[test]
    fn test_grouped_cycle() {
        // *5m/(plan,review,complaints)
        let res = parse_chain("*5m/(plan,review,complaints)").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: None,
                    ceiling: None,
                    floor: None,
                    repeat: Some(Repeat::EveryMinutes(5))
                },
                Box::new(Node::Cycle(vec![
                    Node::Job("plan".to_string()),
                    Node::Job("review".to_string()),
                    Node::Job("complaints".to_string())
                ]))
            )
        );
    }

    #[test]
    fn test_nested_grouped_cycles() {
        // *5/(<5/plan,>1/complaints)
        let res = parse_chain("*5/(<5/plan,>1/complaints)").unwrap();
        assert_eq!(
            res.root,
            Node::Wrap(
                Modifier {
                    width: None,
                    ceiling: None,
                    floor: None,
                    repeat: Some(Repeat::Count(5))
                },
                Box::new(Node::Cycle(vec![
                    Node::Wrap(
                        Modifier {
                            width: None,
                            ceiling: Some(5),
                            floor: None,
                            repeat: None
                        },
                        Box::new(Node::Job("plan".to_string()))
                    ),
                    Node::Wrap(
                        Modifier {
                            width: None,
                            ceiling: None,
                            floor: Some(1),
                            repeat: None
                        },
                        Box::new(Node::Job("complaints".to_string()))
                    )
                ]))
            )
        );
    }

    // ========== Disambiguation ==========

    #[test]
    fn test_directive_line_with_space() {
        assert!(is_directive_line("<< plan"));
        assert!(is_directive_line("  << plan  "));
    }

    #[test]
    fn test_variable_substitution_no_space() {
        // <<plan>> is a variable substitution, not a directive
        assert!(!is_directive_line("<<plan>>"));
    }

    #[test]
    fn test_variable_substitution_no_space_trimmed() {
        assert!(!is_directive_line("  <<plan>>  "));
    }

    #[test]
    fn test_directive_vs_var_mixed() {
        // A line with both directive and var: the directive check only cares
        // about the `^<<\s+` pattern
        assert!(is_directive_line("<< plan and more text"));
    }

    // ========== Error Cases ==========

    #[test]
    fn test_duplicate_x_axis() {
        let err = parse_chain("x5x2/plan");
        assert!(matches!(
            err,
            Err(ParseError::DuplicateAxis { axis: "x", .. })
        ));
    }

    #[test]
    fn test_duplicate_count_axis_two_ceiling() {
        let err = parse_chain("<3<5/plan");
        assert!(matches!(
            err,
            Err(ParseError::DuplicateAxis { axis: "count", .. })
        ));
    }

    #[test]
    fn test_duplicate_count_axis_floor_then_ceiling() {
        let result = parse_chain(">3<5/plan");
        // Actually this should parse as a range, so it's OK.
        assert!(result.is_ok());
        // This is the range case, not a duplicate.
    }

    #[test]
    fn test_bad_count_zero() {
        let err = parse_chain("x0/plan");
        assert!(matches!(err, Err(ParseError::BadCount { .. })));
    }

    #[test]
    fn test_bad_count_non_numeric() {
        let err = parse_chain("xa/plan");
        assert!(matches!(err, Err(ParseError::BadCount { .. })));
    }

    #[test]
    fn test_trailing_slash() {
        let err = parse_chain("x5/");
        assert!(matches!(err, Err(ParseError::TrailingSlash { .. })));
    }

    #[test]
    fn test_empty_group() {
        // Empty group as a target (not as a count)
        let err = parse_chain("x1/()");
        assert!(matches!(err, Err(ParseError::EmptyGroup { .. })));
    }

    #[test]
    fn test_unbalanced_parens_open() {
        let err = parse_chain("x5/(plan,review");
        assert!(matches!(err, Err(ParseError::UnbalancedParens { .. })));
    }

    #[test]
    fn test_empty_chain() {
        let err = parse_chain("");
        assert!(matches!(err, Err(ParseError::Empty)));
    }

    #[test]
    fn test_whitespace_only() {
        let err = parse_chain("   ");
        assert!(matches!(err, Err(ParseError::Empty)));
    }

    #[test]
    fn test_unexpected_char_hash() {
        let err = parse_chain("plan#review");
        assert!(matches!(err, Err(ParseError::UnexpectedChar { .. })));
    }

    // ========== directives() extraction ==========

    #[test]
    fn test_directives_single() {
        let body = "Some text\n<< plan\nMore text";
        let dirs = directives(body);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].0, 1);
        assert!(dirs[0].1.is_ok());
    }

    #[test]
    fn test_directives_multiple() {
        let body = "<< plan\nSome text\n<< implement\nMore";
        let dirs = directives(body);
        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0].0, 0);
        assert_eq!(dirs[1].0, 2);
    }

    #[test]
    fn test_directives_ignores_non_directives() {
        let body = "normal line\n<< plan\n<<var>> substitution\n<< implement";
        let dirs = directives(body);
        assert_eq!(dirs.len(), 2); // Only the two with space
    }

    #[test]
    fn test_directives_parse_error_captured() {
        let body = "<< x5x2/plan";
        let dirs = directives(body);
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].1.is_err());
    }

    #[test]
    fn test_directives_empty_directive_error() {
        let body = "<< ";
        let dirs = directives(body);
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].1.is_err());
    }

    // ========== Equivalence: prefix ↔ directive ==========

    #[test]
    fn test_equivalence_bare_job() {
        let prefix = "plan";
        let directive_line = format!("<< {}", prefix);
        let prefix_parsed = parse_chain(prefix).unwrap();
        let dirs = directives(&directive_line);
        let directive_parsed = dirs[0].1.as_ref().unwrap();
        assert_eq!(prefix_parsed.root, directive_parsed.root);
    }

    #[test]
    fn test_equivalence_complex_chain() {
        let prefix = "x5/plan, implement";
        let directive_line = format!("<< {}", prefix);
        let prefix_parsed = parse_chain(prefix).unwrap();
        let dirs = directives(&directive_line);
        let directive_parsed = dirs[0].1.as_ref().unwrap();
        assert_eq!(prefix_parsed.root, directive_parsed.root);
    }
}
