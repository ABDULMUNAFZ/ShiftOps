//! Minimal version handling for runtime declarations: extract major
//! versions from exact declarations and test whether a major version can
//! satisfy an npm-style range (`>=18 <21`, `^20.11`, `~18.17.0`, `18.x`,
//! `16 || 18`). Unknown syntax yields `None` rather than a guess.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

fn parse_partial(s: &str) -> Option<(Option<u64>, Option<u64>, Option<u64>)> {
    let s = s.trim().trim_start_matches(['v', 'V', '=']);
    if s.is_empty() {
        return None;
    }
    let mut parts = s.split('.');
    let num = |p: Option<&str>| -> Option<Option<u64>> {
        match p {
            None => Some(None),
            Some(x) if x.eq_ignore_ascii_case("x") || x == "*" => Some(None),
            Some(x) => {
                let digits: String = x.chars().take_while(char::is_ascii_digit).collect();
                if digits.is_empty() {
                    None
                } else {
                    digits.parse().ok().map(Some)
                }
            }
        }
    };
    let major = num(parts.next())?;
    major?;
    let minor = num(parts.next())?;
    let patch = num(parts.next())?;
    Some((major, minor, patch))
}

/// Major version of an exact-ish declaration: `20`, `v20.11.0`, `20.11`,
/// `20-alpine`, `node:20-bookworm-slim` tags. `lts/*`, `latest`, `node`
/// and other aliases return `None`.
pub fn major_of(declaration: &str) -> Option<u64> {
    let d = declaration.trim().trim_matches(['"', '\'']);
    let d = d.split(['-', '@']).next().unwrap_or(d);
    let (major, _, _) = parse_partial(d)?;
    major
}

#[derive(Debug, Clone, Copy)]
enum Op {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
}

fn comparator(token: &str) -> Option<Vec<(Op, Version)>> {
    let (op, rest) = if let Some(r) = token.strip_prefix(">=") {
        (Some(Op::Gte), r)
    } else if let Some(r) = token.strip_prefix("<=") {
        (Some(Op::Lte), r)
    } else if let Some(r) = token.strip_prefix('>') {
        (Some(Op::Gt), r)
    } else if let Some(r) = token.strip_prefix('<') {
        (Some(Op::Lt), r)
    } else if let Some(r) = token.strip_prefix('^') {
        let (ma, mi, pa) = parse_partial(r)?;
        let ma = ma?;
        let lo = Version(ma, mi.unwrap_or(0), pa.unwrap_or(0));
        let hi = if ma > 0 {
            Version(ma + 1, 0, 0)
        } else {
            Version(0, mi.unwrap_or(0) + 1, 0)
        };
        return Some(vec![(Op::Gte, lo), (Op::Lt, hi)]);
    } else if let Some(r) = token.strip_prefix('~') {
        let (ma, mi, pa) = parse_partial(r)?;
        let ma = ma?;
        let lo = Version(ma, mi.unwrap_or(0), pa.unwrap_or(0));
        let hi = match mi {
            Some(mi) => Version(ma, mi + 1, 0),
            None => Version(ma + 1, 0, 0),
        };
        return Some(vec![(Op::Gte, lo), (Op::Lt, hi)]);
    } else {
        (None, token)
    };
    let (ma, mi, pa) = parse_partial(rest)?;
    let ma = ma?;
    match op {
        Some(op) => Some(vec![(op, Version(ma, mi.unwrap_or(0), pa.unwrap_or(0)))]),
        // Bare `18`, `18.x`, `18.17` mean the whole partial range.
        None => match (mi, pa) {
            (Some(mi), Some(pa)) => Some(vec![(Op::Eq, Version(ma, mi, pa))]),
            (Some(mi), None) => Some(vec![
                (Op::Gte, Version(ma, mi, 0)),
                (Op::Lt, Version(ma, mi + 1, 0)),
            ]),
            _ => Some(vec![
                (Op::Gte, Version(ma, 0, 0)),
                (Op::Lt, Version(ma + 1, 0, 0)),
            ]),
        },
    }
}

fn satisfies(v: Version, set: &[(Op, Version)]) -> bool {
    set.iter().all(|(op, b)| match op {
        Op::Gt => v > *b,
        Op::Gte => v >= *b,
        Op::Lt => v < *b,
        Op::Lte => v <= *b,
        Op::Eq => v == *b,
    })
}

/// Parsed range: a list of alternatives (`||`), each a list of comparators.
fn parse_range(range: &str) -> Option<Vec<Vec<(Op, Version)>>> {
    let range = range.trim();
    if range.is_empty() || range == "*" || range.eq_ignore_ascii_case("x") {
        return Some(vec![vec![]]);
    }
    let mut alternatives = Vec::new();
    for alt in range.split("||") {
        // Hyphen ranges: `18 - 20`.
        if let Some((lo, hi)) = alt.split_once(" - ") {
            let mut set = comparator(&format!(">={}", lo.trim()))?;
            let (ma, mi, pa) = parse_partial(hi)?;
            let ma = ma?;
            set.push(match (mi, pa) {
                (Some(mi), Some(pa)) => (Op::Lte, Version(ma, mi, pa)),
                (Some(mi), None) => (Op::Lt, Version(ma, mi + 1, 0)),
                _ => (Op::Lt, Version(ma + 1, 0, 0)),
            });
            alternatives.push(set);
            continue;
        }
        let mut set = Vec::new();
        let normalized = alt
            .replace(">= ", ">=")
            .replace("<= ", "<=")
            .replace("> ", ">")
            .replace("< ", "<");
        for token in normalized.split_whitespace() {
            set.extend(comparator(token)?);
        }
        alternatives.push(set);
    }
    Some(alternatives)
}

/// Can some release of `major` satisfy `range`? `None` when the range syntax is not understood.
pub fn major_can_satisfy(major: u64, range: &str) -> Option<bool> {
    let alternatives = parse_range(range)?;
    let mut candidates = vec![Version(major, 0, 0), Version(major, 999, 999)];
    for set in &alternatives {
        for (_, b) in set {
            if b.0 == major {
                candidates.push(*b);
                candidates.push(Version(b.0, b.1, b.2.saturating_add(1)));
            }
        }
    }
    Some(
        alternatives
            .iter()
            .any(|set| candidates.iter().any(|v| satisfies(*v, set))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn majors_from_declarations() {
        assert_eq!(major_of("20"), Some(20));
        assert_eq!(major_of("v18.17.1"), Some(18));
        assert_eq!(major_of("20-alpine"), Some(20));
        assert_eq!(major_of("20.11-bookworm-slim"), Some(20));
        assert_eq!(major_of("'18.x'"), Some(18));
        assert_eq!(major_of("lts/*"), None);
        assert_eq!(major_of("latest"), None);
        assert_eq!(major_of("node"), None);
    }

    #[test]
    fn ranges() {
        assert_eq!(major_can_satisfy(20, ">=18"), Some(true));
        assert_eq!(major_can_satisfy(16, ">=18"), Some(false));
        assert_eq!(major_can_satisfy(20, ">=18 <20"), Some(false));
        assert_eq!(major_can_satisfy(18, ">= 18.17.0 < 19"), Some(true));
        assert_eq!(major_can_satisfy(20, "^20.11"), Some(true));
        assert_eq!(major_can_satisfy(21, "^20.11"), Some(false));
        assert_eq!(major_can_satisfy(18, "~18.17.0"), Some(true));
        assert_eq!(major_can_satisfy(18, "16 || 18"), Some(true));
        assert_eq!(major_can_satisfy(20, "16 || 18"), Some(false));
        assert_eq!(major_can_satisfy(19, "18 - 20"), Some(true));
        assert_eq!(major_can_satisfy(20, "18.x"), Some(false));
        assert_eq!(major_can_satisfy(22, "*"), Some(true));
        assert_eq!(major_can_satisfy(20, "lts"), None);
    }
}
