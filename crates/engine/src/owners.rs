//! GitHub CODEOWNERS: who is likely to review a changed or impacted file.
//!
//! Ownership is routing metadata, not authorization: Ripplepath never decides who may approve
//! anything. Semantics follow GitHub's documentation: gitignore-style patterns, the *last* matching
//! line wins, `!` negation, `[ ]` ranges and `\#` escapes are unsupported (such lines are reported as
//! errors and skipped, as GitHub does). Pure apart from nothing: the caller reads the file.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Searched in this order; the first that exists is used (GitHub's order).
pub const CODEOWNERS_PATHS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];
/// GitHub ignores CODEOWNERS files above 3 MB.
pub const MAX_CODEOWNERS_BYTES: u64 = 3 * 1024 * 1024;
const MAX_LISTED_ERRORS: usize = 50;
const MAX_LISTED_FILES: usize = 500;
const MAX_PATTERN_SEGMENTS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    /// `**`: zero or more whole path segments.
    Any,
    /// A glob for one segment (`*` and `?` never cross `/`).
    Glob(Vec<char>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pattern {
    segments: Vec<Segment>,
    /// Trailing `/`: matches only paths *inside* a matching directory.
    dir_only: bool,
    /// Last segment is exactly `*` (`docs/*`): direct children only, per GitHub's documentation.
    children_only: bool,
}

impl Pattern {
    fn parse(raw: &str) -> Result<Self, String> {
        if raw.starts_with('!') {
            return Err("negated patterns ('!') are not supported by CODEOWNERS".to_owned());
        }
        if raw.contains('[') || raw.contains(']') {
            return Err("character ranges ('[ ]') are not supported by CODEOWNERS".to_owned());
        }
        let dir_only = raw.ends_with('/');
        let trimmed = raw.trim_end_matches('/');
        // A slash at the start or in the middle anchors the pattern to the repository root;
        // otherwise it matches at any depth (gitignore rules).
        let anchored = trimmed.contains('/');
        let body = trimmed.trim_start_matches('/');
        if body.is_empty() {
            // `/` alone: everything.
            return Ok(Self { segments: vec![Segment::Any], dir_only: false, children_only: false });
        }
        let mut segments = Vec::new();
        if !anchored {
            segments.push(Segment::Any);
        }
        for part in body.split('/') {
            match part {
                "" => continue,
                "**" => {
                    if segments.last() != Some(&Segment::Any) {
                        segments.push(Segment::Any);
                    }
                }
                glob => segments.push(Segment::Glob(glob.chars().collect())),
            }
        }
        if segments.len() > MAX_PATTERN_SEGMENTS {
            return Err(format!("pattern has more than {MAX_PATTERN_SEGMENTS} segments"));
        }
        let children_only = matches!(segments.last(), Some(Segment::Glob(g)) if g == &['*']);
        Ok(Self { segments, dir_only, children_only })
    }

    fn matches(&self, path: &str) -> bool {
        let parts: Vec<&str> = path.split('/').collect();
        let prefixes = match_prefixes(&self.segments, &parts);
        let full = prefixes[parts.len()];
        if !self.dir_only && full {
            return true;
        }
        if self.children_only {
            return false;
        }
        // A pattern naming a directory owns everything below it: a proper prefix matches.
        prefixes[1..parts.len()].iter().any(|m| *m)
    }
}

/// Dynamic programming over (pattern segment, path segment); `**` matches zero or more segments,
/// except that a trailing `**` must match at least one (`docs/**` is the contents of `docs`).
/// Returns, for every `k`, whether the pattern matches `path[..k]`: one pass answers both the full
/// match and the directory-prefix matches, keeping the cost at O(pattern × path).
fn match_prefixes(pattern: &[Segment], path: &[&str]) -> Vec<bool> {
    let (p, s) = (pattern.len(), path.len());
    // reachable[i][j]: pattern[..i] matches path[..j].
    let mut reachable = vec![vec![false; s + 1]; p + 1];
    reachable[0][0] = true;
    for i in 1..=p {
        for j in 0..=s {
            reachable[i][j] = match &pattern[i - 1] {
                Segment::Any => {
                    let trailing = i == p;
                    (!trailing && reachable[i - 1][j]) || (j > 0 && (reachable[i - 1][j - 1] || reachable[i][j - 1]))
                }
                Segment::Glob(glob) => j > 0 && reachable[i - 1][j - 1] && match_glob(glob, path[j - 1]),
            };
        }
    }
    reachable.swap_remove(p)
}

/// Iterative wildcard matching with single-star backtracking: linear-ish, no recursion.
fn match_glob(glob: &[char], text: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    let (mut g, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if g < glob.len() && (glob[g] == '?' || glob[g] == text[t]) {
            g += 1;
            t += 1;
        } else if g < glob.len() && glob[g] == '*' {
            star = Some((g, t));
            g += 1;
        } else if let Some((sg, st)) = star {
            g = sg + 1;
            t = st + 1;
            star = Some((sg, st + 1));
        } else {
            return false;
        }
    }
    glob[g..].iter().all(|c| *c == '*')
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OwnersParseError {
    pub line: u32,
    pub message: String,
}

#[derive(Clone, Debug)]
struct OwnerRule {
    line: u32,
    pattern: Pattern,
    owners: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CodeOwners {
    rules: Vec<OwnerRule>,
    pub errors: Vec<OwnersParseError>,
}

/// Owners of one path: the last matching line, which may list no owners (explicitly unowned).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ownership<'a> {
    pub line: u32,
    pub owners: &'a [String],
}

fn valid_owner(owner: &str) -> bool {
    // `@user`, `@org/team`, or an email address.
    let handle = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
    match owner.strip_prefix('@') {
        Some(rest) => handle(rest),
        None => owner.split_once('@').is_some_and(|(user, domain)| !user.is_empty() && domain.contains('.')),
    }
}

impl CodeOwners {
    pub fn parse(text: &str) -> Self {
        let mut parsed = Self::default();
        for (index, line) in text.lines().enumerate() {
            let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut tokens = line.split_whitespace().take_while(|t| !t.starts_with('#'));
            let Some(raw) = tokens.next() else { continue };
            let owners: Vec<String> = tokens.map(str::to_owned).collect();
            let result = Pattern::parse(raw).and_then(|pattern| match owners.iter().find(|o| !valid_owner(o)) {
                Some(bad) => Err(format!("invalid owner {bad:?}")),
                None => Ok(pattern),
            });
            match result {
                Ok(pattern) => parsed.rules.push(OwnerRule { line: number, pattern, owners }),
                Err(message) => parsed.errors.push(OwnersParseError { line: number, message }),
            }
        }
        parsed
    }

    /// Last matching rule wins.
    pub fn owners_of(&self, path: &str) -> Option<Ownership<'_>> {
        self.rules.iter().rev().find(|r| r.pattern.matches(path)).map(|r| Ownership { line: r.line, owners: &r.owners })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileRole {
    Changed,
    Impacted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOwners {
    pub path: String,
    pub role: FileRole,
    /// Sorted as written in CODEOWNERS; empty when unowned.
    pub owners: Vec<String>,
    /// CODEOWNERS line that decided ownership.
    pub line: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerSummary {
    pub owner: String,
    pub changed_files: usize,
    pub impacted_files: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnersReport {
    /// CODEOWNERS file read from the head revision; `None` when there is none.
    pub source: Option<String>,
    /// The change edits the CODEOWNERS file itself.
    pub changed_in_head: bool,
    /// Sorted by (role, path); at most 500.
    pub files: Vec<FileOwners>,
    pub files_truncated: bool,
    /// Sorted by owner.
    pub owners: Vec<OwnerSummary>,
    pub unowned_changed_files: usize,
    pub errors: Vec<OwnersParseError>,
    pub note: String,
}

pub const OWNERSHIP_NOTE: &str =
    "Ownership is review-routing metadata from CODEOWNERS, not authorization; Ripplepath never decides approvals.";

/// Builds the report for changed and impacted files. A file that is both is listed as changed.
pub fn report(
    codeowners: Option<(&str, &CodeOwners)>,
    changed_in_head: bool,
    changed_files: &BTreeSet<String>,
    impacted_files: &BTreeSet<String>,
    read_error: Option<String>,
) -> OwnersReport {
    let mut files = Vec::new();
    let mut summary: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut unowned_changed_files = 0;
    let roles = changed_files
        .iter()
        .map(|p| (p, FileRole::Changed))
        .chain(impacted_files.iter().filter(|p| !changed_files.contains(*p)).map(|p| (p, FileRole::Impacted)));
    for (path, role) in roles {
        let ownership = codeowners.and_then(|(_, c)| c.owners_of(path));
        let owners: Vec<String> = ownership.as_ref().map(|o| o.owners.to_vec()).unwrap_or_default();
        if owners.is_empty() && role == FileRole::Changed {
            unowned_changed_files += 1;
        }
        for owner in &owners {
            let entry = summary.entry(owner.clone()).or_default();
            match role {
                FileRole::Changed => entry.0 += 1,
                FileRole::Impacted => entry.1 += 1,
            }
        }
        files.push(FileOwners { path: path.clone(), role, owners, line: ownership.map(|o| o.line) });
    }
    files.sort_by(|a, b| (a.role, &a.path).cmp(&(b.role, &b.path)));
    let files_truncated = files.len() > MAX_LISTED_FILES;
    files.truncate(MAX_LISTED_FILES);
    let mut errors: Vec<OwnersParseError> =
        read_error.map(|message| OwnersParseError { line: 0, message }).into_iter().collect();
    if let Some((_, c)) = codeowners {
        errors.extend(c.errors.iter().cloned());
    }
    errors.truncate(MAX_LISTED_ERRORS);
    OwnersReport {
        source: codeowners.map(|(path, _)| path.to_owned()),
        changed_in_head,
        files,
        files_truncated,
        owners: summary
            .into_iter()
            .map(|(owner, (changed_files, impacted_files))| OwnerSummary { owner, changed_files, impacted_files })
            .collect(),
        unowned_changed_files,
        errors,
        note: OWNERSHIP_NOTE.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn owner(file: &CodeOwners, path: &str) -> Option<Vec<String>> {
        file.owners_of(path).map(|o| o.owners.to_vec())
    }

    fn matches(pattern: &str, path: &str) -> bool {
        Pattern::parse(pattern).unwrap().matches(path)
    }

    #[test]
    fn pattern_semantics_follow_github_documentation() {
        let cases: &[(&str, &str, bool)] = &[
            // Global and extension patterns match at any depth.
            ("*", "README.md", true),
            ("*", "a/b/c.rs", true),
            ("*.js", "app.js", true),
            ("*.js", "src/deep/app.js", true),
            ("*.js", "src/app.ts", false),
            // A trailing slash: anything inside such a directory, anywhere.
            ("build/logs/", "build/logs/a.log", true),
            ("build/logs/", "build/logs/deep/a.log", true),
            ("build/logs/", "x/build/logs/a.log", false),
            ("apps/", "apps/a.js", true),
            ("apps/", "nested/apps/a.js", true),
            ("apps/", "apps", false),
            // `docs/*` owns direct children only.
            ("docs/*", "docs/getting-started.md", true),
            ("docs/*", "docs/build-app/troubleshooting.md", false),
            ("docs/*", "x/docs/a.md", false),
            // Leading slash anchors to the root; a directory pattern owns its contents.
            ("/docs/", "docs/a.md", true),
            ("/docs/", "x/docs/a.md", false),
            ("/apps/github", "apps/github/x.rb", true),
            ("/apps/github", "apps/github", true),
            ("/apps/github", "apps/githubber/x.rb", false),
            // `**`.
            ("**/logs", "logs/a", true),
            ("**/logs", "deeply/nested/logs/a.log", true),
            ("**/logs", "deeply/nested/logs", true),
            ("docs/**", "docs/a/b.md", true),
            ("docs/**", "docs", false),
            ("a/**/b", "a/b", true),
            ("a/**/b", "a/x/y/b", true),
            ("a/**/b", "a/x/y/c", false),
            // A middle slash anchors; no leading slash needed.
            ("src/main", "src/main/A.java", true),
            ("src/main", "lib/src/main/A.java", false),
            // Unanchored names match files or directories at any depth.
            ("Makefile", "tools/Makefile", true),
            ("vendor", "a/vendor/lib.js", true),
            // `?` and `*` stay inside one segment; matching is case-sensitive.
            ("src/?.ts", "src/a.ts", true),
            ("src/?.ts", "src/ab.ts", false),
            ("src/*.ts", "src/a/b.ts", false),
            ("*.JS", "app.js", false),
        ];
        for (pattern, path, expected) in cases {
            assert_eq!(matches(pattern, path), *expected, "{pattern} vs {path}");
        }
    }

    #[test]
    fn last_matching_line_wins_and_empty_owner_lists_unown() {
        let file = CodeOwners::parse(
            "# comment\n* @global\n*.js @js-owner # inline comment\n/apps/ @octocat\n/apps/github\n\n/build/ @a @org/team ops@example.com\n",
        );
        assert!(file.errors.is_empty(), "{:?}", file.errors);
        assert_eq!(owner(&file, "README.md"), Some(vec!["@global".to_owned()]));
        assert_eq!(owner(&file, "src/app.js"), Some(vec!["@js-owner".to_owned()]));
        assert_eq!(owner(&file, "apps/x.js"), Some(vec!["@octocat".to_owned()]), "later line beats *.js");
        assert_eq!(owner(&file, "apps/github/x.js"), Some(vec![]), "explicitly unowned");
        assert_eq!(file.owners_of("apps/github/x.js").unwrap().line, 5);
        assert_eq!(
            owner(&file, "build/out.txt"),
            Some(vec!["@a".to_owned(), "@org/team".to_owned(), "ops@example.com".to_owned()])
        );
        assert_eq!(CodeOwners::parse("/docs/ @d\n").owners_of("src/a.rs"), None);
    }

    #[test]
    fn unsupported_syntax_is_reported_and_skipped() {
        let file = CodeOwners::parse("!secret @a\nsrc/[ab].rs @b\nlib/ not-an-owner\n* @ok\n");
        let lines: Vec<u32> = file.errors.iter().map(|e| e.line).collect();
        assert_eq!(lines, vec![1, 2, 3]);
        assert_eq!(owner(&file, "lib/x"), Some(vec!["@ok".to_owned()]));
    }

    #[test]
    fn report_groups_changed_and_impacted_files() {
        let file = CodeOwners::parse("* @all\n/api/ @api-team\n/gen/\n");
        let changed: BTreeSet<String> = ["api/A.java", "gen/G.java"].map(str::to_owned).into();
        let impacted: BTreeSet<String> = ["api/A.java", "domain/D.java"].map(str::to_owned).into();
        let report = report(Some((".github/CODEOWNERS", &file)), false, &changed, &impacted, None);
        let rows: Vec<(&str, FileRole, usize)> =
            report.files.iter().map(|f| (f.path.as_str(), f.role, f.owners.len())).collect();
        assert_eq!(
            rows,
            vec![
                ("api/A.java", FileRole::Changed, 1),
                ("gen/G.java", FileRole::Changed, 0),
                ("domain/D.java", FileRole::Impacted, 1)
            ]
        );
        assert_eq!(report.unowned_changed_files, 1);
        assert_eq!(
            report.owners,
            vec![
                OwnerSummary { owner: "@all".into(), changed_files: 0, impacted_files: 1 },
                OwnerSummary { owner: "@api-team".into(), changed_files: 1, impacted_files: 0 },
            ]
        );
    }

    #[test]
    fn pathological_patterns_stay_fast() {
        let pattern = format!("{}x", "**/a*".repeat(100));
        let path = "a/".repeat(200) + "b";
        let started = std::time::Instant::now();
        assert!(!matches(&pattern, &path));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
