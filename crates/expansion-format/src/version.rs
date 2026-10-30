//! Semantic versions and version ranges: a pack's `version`, its
//! `min_engine`, and the ranges a `requires` / `conflicts` entry puts on
//! another pack (`base@^1.2`).
//!
//! A [`Version`] is `MAJOR.MINOR.PATCH[-pre][+build]`; precedence follows
//! semver 2.0 (build metadata is ignored, a pre-release sorts before its
//! release, numeric identifiers compare as numbers and before alphanumeric
//! ones).
//!
//! A [`VersionReq`] is one or more comparators, all of which must hold,
//! separated by commas or whitespace (`>=1.2.0, <2.0.0` or `>=1.2 <2`). The
//! operators are cargo's:
//!
//! | Form | Means |
//! |---|---|
//! | `1.2.3`, `^1.2.3` | `>=1.2.3, <2.0.0` (`^0.2.3` is `<0.3.0`, `^0.0.3` is `=0.0.3`) |
//! | `~1.2.3` | `>=1.2.3, <1.3.0` |
//! | `=1.2`, `1.2.*` | any `1.2.x` |
//! | `>1.2`, `>=1.2`, `<1.2`, `<=1.2` | bounds; a missing part reads as the whole range of it |
//! | `*` | anything |
//!
//! A pre-release version satisfies a range only when one of its comparators
//! names a pre-release of the same `MAJOR.MINOR.PATCH`, so `^1.0` never picks
//! up `2.0.0-alpha`.

use std::cmp::Ordering;
use std::fmt;

/// A parsed semantic version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Pre-release identifiers (`alpha.1` -> `["alpha", "1"]`); empty for a
    /// release.
    pub pre: Vec<String>,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: Vec::new(),
        }
    }

    /// Parse full semver text. `None` for anything else (`1.0`, `01.0.0`).
    pub fn parse(v: &str) -> Option<Self> {
        let rest = match v.split_once('+') {
            Some((r, b)) => {
                ident_list(b)?;
                r
            }
            None => v,
        };
        let (core, pre) = match rest.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (rest, None),
        };
        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        let pre = match pre {
            Some(p) => ident_list(p)?,
            None => Vec::new(),
        };
        Some(Self {
            major: numeric(parts[0])?,
            minor: numeric(parts[1])?,
            patch: numeric(parts[2])?,
            pre,
        })
    }

    fn triple(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.triple()
            .cmp(&other.triple())
            .then_with(|| cmp_pre(&self.pre, &other.pre))
    }
}

fn cmp_pre(a: &[String], b: &[String]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    for (x, y) in a.iter().zip(b) {
        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(m), Ok(n)) => m.cmp(&n),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            _ => x.cmp(y),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

fn numeric(s: &str) -> Option<u64> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0')))
        .then(|| s.parse().ok())
        .flatten()
}

fn ident_list(s: &str) -> Option<Vec<String>> {
    s.split('.')
        .map(|part| {
            (!part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
                .then(|| part.to_string())
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Caret,
    Tilde,
    Exact,
    Gt,
    Ge,
    Lt,
    Le,
}

/// One comparator; `minor` / `patch` are `None` where the text left them
/// out.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Comparator {
    op: Op,
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Vec<String>,
}

impl Comparator {
    fn parse(text: &str) -> Result<Option<Self>, String> {
        if text == "*" {
            return Ok(None);
        }
        let (op, rest) = [
            (">=", Op::Ge),
            ("<=", Op::Le),
            (">", Op::Gt),
            ("<", Op::Lt),
            ("=", Op::Exact),
            ("^", Op::Caret),
            ("~", Op::Tilde),
        ]
        .iter()
        .find_map(|(p, op)| text.strip_prefix(p).map(|r| (*op, r)))
        .unwrap_or((Op::Caret, text));
        let bad = || format!("{text:?} is not a version comparator");
        let (core, pre) = match rest.split_once('-') {
            Some((c, p)) => (c, ident_list(p).ok_or_else(bad)?),
            None => (rest, Vec::new()),
        };
        let mut parts = core.split('.');
        let mut wild = false;
        let mut part = |required: bool| -> Result<Option<u64>, String> {
            match parts.next() {
                None if !required => Ok(None),
                Some("*" | "x" | "X") if !required => {
                    wild = true;
                    Ok(None)
                }
                Some(p) => numeric(p).map(Some).ok_or_else(bad),
                None => Err(bad()),
            }
        };
        let major = part(true)?.ok_or_else(bad)?;
        let minor = part(false)?;
        let patch = if minor.is_some() { part(false)? } else { None };
        if parts.next().is_some() || (!pre.is_empty() && patch.is_none()) {
            return Err(bad());
        }
        // `1.2.*` reads as `=1.2`.
        let op = if wild && op == Op::Caret {
            Op::Exact
        } else {
            op
        };
        Ok(Some(Self {
            op,
            major,
            minor,
            patch,
            pre,
        }))
    }

    /// The lowest version the comparator's own text names.
    fn floor(&self) -> Version {
        Version {
            major: self.major,
            minor: self.minor.unwrap_or(0),
            patch: self.patch.unwrap_or(0),
            pre: self.pre.clone(),
        }
    }

    /// The first version past the range the text names (`1.2` -> `1.3.0`).
    fn past(&self) -> Version {
        match (self.minor, self.patch) {
            (None, _) => Version::new(self.major + 1, 0, 0),
            (Some(m), None) => Version::new(self.major, m + 1, 0),
            (Some(m), Some(p)) => Version::new(self.major, m, p + 1),
        }
    }

    fn matches(&self, v: &Version) -> bool {
        let lo = self.floor();
        match self.op {
            Op::Exact => match self.patch {
                Some(_) => *v == lo,
                None => *v >= lo && *v < self.past(),
            },
            Op::Gt => match self.patch {
                Some(_) => *v > lo,
                None => *v >= self.past(),
            },
            Op::Ge => *v >= lo,
            Op::Lt => *v < lo,
            Op::Le => match self.patch {
                Some(_) => *v <= lo,
                None => *v < self.past(),
            },
            Op::Tilde => {
                let hi = match self.minor {
                    Some(m) => Version::new(self.major, m + 1, 0),
                    None => Version::new(self.major + 1, 0, 0),
                };
                *v >= lo && *v < hi
            }
            Op::Caret => {
                let hi = match (self.major, self.minor, self.patch) {
                    (0, None, _) => Version::new(1, 0, 0),
                    (0, Some(0), None) => Version::new(0, 1, 0),
                    (0, Some(0), Some(p)) => Version::new(0, 0, p + 1),
                    (0, Some(m), _) => Version::new(0, m + 1, 0),
                    (maj, _, _) => Version::new(maj + 1, 0, 0),
                };
                *v >= lo && *v < hi
            }
        }
    }
}

/// A version range: every comparator must hold. `*` (or an empty
/// comparator list) matches every release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    comparators: Vec<Comparator>,
    text: String,
}

impl VersionReq {
    /// Parse a range. The error says which comparator is malformed.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("the version range is empty".into());
        }
        let mut comparators = Vec::new();
        // `>= 1.2` with a space: glue an operator token to what follows.
        let mut pending_op = String::new();
        for tok in text.split([',', ' ', '\t']).filter(|t| !t.is_empty()) {
            if tok.chars().all(|c| "<>=^~".contains(c)) {
                pending_op.push_str(tok);
                continue;
            }
            let tok = format!("{pending_op}{tok}");
            pending_op.clear();
            if let Some(c) = Comparator::parse(&tok)? {
                comparators.push(c);
            }
        }
        if !pending_op.is_empty() {
            return Err(format!("{pending_op:?} has no version after it"));
        }
        Ok(Self {
            comparators,
            text: text.to_string(),
        })
    }

    /// Does `v` satisfy the range.
    pub fn matches(&self, v: &Version) -> bool {
        if !v.pre.is_empty()
            && !self
                .comparators
                .iter()
                .any(|c| !c.pre.is_empty() && c.floor().triple() == v.triple())
        {
            return false;
        }
        self.comparators.iter().all(|c| c.matches(v))
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("{s}"))
    }

    fn holds(req: &str, ver: &str) -> bool {
        VersionReq::parse(req).unwrap().matches(&v(ver))
    }

    #[test]
    fn precedence() {
        let order = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.2.0",
            "1.10.0",
            "2.0.0",
        ];
        for w in order.windows(2) {
            assert!(v(w[0]) < v(w[1]), "{} < {}", w[0], w[1]);
        }
        assert_eq!(v("1.0.0+a"), v("1.0.0+b"), "build metadata is ignored");
        assert!(Version::parse("1.0").is_none());
        assert!(Version::parse("01.0.0").is_none());
    }

    #[test]
    fn ranges() {
        let cases: &[(&str, &[&str], &[&str])] = &[
            ("1.2.3", &["1.2.3", "1.9.0"], &["1.2.2", "2.0.0"]),
            ("^0.2.3", &["0.2.3", "0.2.9"], &["0.3.0", "0.2.2"]),
            ("^0.0.3", &["0.0.3"], &["0.0.4"]),
            ("^0", &["0.0.1", "0.9.9"], &["1.0.0"]),
            ("~1.2.3", &["1.2.3", "1.2.9"], &["1.3.0"]),
            ("~1", &["1.0.0", "1.9.9"], &["2.0.0"]),
            ("=1.2", &["1.2.0", "1.2.7"], &["1.3.0", "1.1.9"]),
            ("1.2.*", &["1.2.0", "1.2.7"], &["1.3.0"]),
            (">1.2", &["1.3.0"], &["1.2.9"]),
            (">1.2.0", &["1.2.1"], &["1.2.0"]),
            ("<=1.2", &["1.2.9"], &["1.3.0"]),
            ("<1.2", &["1.1.9"], &["1.2.0"]),
            (">=1.2, <2", &["1.2.0", "1.99.0"], &["2.0.0", "1.1.0"]),
            (">=1.2 <2.0.0", &["1.5.0"], &["2.0.0"]),
            (">= 1.2", &["1.2.0"], &["1.1.0"]),
            ("*", &["0.0.1", "9.9.9"], &[]),
        ];
        for (req, yes, no) in cases {
            for y in *yes {
                assert!(holds(req, y), "{req} should hold {y}");
            }
            for n in *no {
                assert!(!holds(req, n), "{req} should refuse {n}");
            }
        }
    }

    #[test]
    fn pre_releases_need_a_pre_release_comparator() {
        assert!(!holds("^1.0", "2.0.0-alpha"));
        assert!(!holds(">=1.0.0", "1.5.0-rc.1"));
        assert!(holds(">=1.5.0-rc.1", "1.5.0-rc.2"));
        assert!(!holds(">=1.5.0-rc.1", "1.6.0-rc.1"));
    }

    #[test]
    fn malformed_ranges_say_why() {
        for bad in ["", ">=", "1.2.3.4", "abc", "^01.2", "1.2-rc", ">=1.x.3"] {
            assert!(VersionReq::parse(bad).is_err(), "{bad:?}");
        }
    }
}
