//! `ripplepath.yml`: layers, architecture rules, critical and generated paths, test mode, policy.
//!
//! Parsing is strict (`deny_unknown_fields` everywhere): a misspelled key in a policy file must be
//! an error, never a silently ignored gate. The file is read from the *base* revision by the
//! analysis (docs/adr/0005-config-from-base-revision.md); this module only parses and validates.

use std::collections::BTreeSet;

use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::report::SelectionMode;

/// Repository-relative location of the configuration file.
pub const CONFIG_PATH: &str = "ripplepath.yml";
/// A policy file is a few hundred lines at most; anything larger is refused before parsing.
pub const MAX_CONFIG_BYTES: u64 = 64 * 1024;
pub const MAX_LAYERS: usize = 64;
pub const MAX_RULES: usize = 256;
pub const MAX_PATTERNS: usize = 1024;
const MAX_NAME_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("{CONFIG_PATH} is {size} bytes, above the {MAX_CONFIG_BYTES}-byte limit")]
    TooLarge { size: u64 },
    #[error("{CONFIG_PATH} is not UTF-8 text")]
    NotText,
    #[error("{CONFIG_PATH}: {0}")]
    Syntax(String),
    #[error("{CONFIG_PATH}: {0}")]
    Invalid(String),
}

/// Merge-policy gates. Names are the identifiers used in `policy.fail_on` / `policy.warn_on` and in
/// the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    /// A dependency that breaks a layer rule exists in head but not in base.
    NewArchitectureViolation,
    /// Any rule-breaking dependency exists in head, including pre-existing ones.
    ArchitectureViolation,
    /// A layer cycle exists in head that did not exist in base.
    NewCycle,
    /// A changed file could not be parsed (or only partially) in head.
    ParseFailureInChangedFile,
    /// A deleted test was located on, or statically reached, a critical path.
    RemovedTestOnCriticalPath,
    /// Public API removed, narrowed, or its signature changed.
    BreakingApiChange,
    /// The head revision changes `ripplepath.yml`; it takes effect only after merge.
    ConfigChanged,
    /// The configuration of base or head is invalid. Always fails; cannot be configured.
    ConfigInvalid,
}

impl Gate {
    pub const CONFIGURABLE: [Gate; 7] = [
        Gate::NewArchitectureViolation,
        Gate::ArchitectureViolation,
        Gate::NewCycle,
        Gate::ParseFailureInChangedFile,
        Gate::RemovedTestOnCriticalPath,
        Gate::BreakingApiChange,
        Gate::ConfigChanged,
    ];

    /// Gates that warn when no policy is configured. `architecture_violation` is excluded: by default
    /// only the delta of a change is its author's concern (pre-existing debt is reported, not gated).
    pub const DEFAULT_WARN: [Gate; 6] = [
        Gate::NewArchitectureViolation,
        Gate::NewCycle,
        Gate::ParseFailureInChangedFile,
        Gate::RemovedTestOnCriticalPath,
        Gate::BreakingApiChange,
        Gate::ConfigChanged,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Gate::NewArchitectureViolation => "new_architecture_violation",
            Gate::ArchitectureViolation => "architecture_violation",
            Gate::NewCycle => "new_cycle",
            Gate::ParseFailureInChangedFile => "parse_failure_in_changed_file",
            Gate::RemovedTestOnCriticalPath => "removed_test_on_critical_path",
            Gate::BreakingApiChange => "breaking_api_change",
            Gate::ConfigChanged => "config_changed",
            Gate::ConfigInvalid => "config_invalid",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CycleMode {
    /// Layer cycles are not computed.
    Off,
    /// Cycles are computed and reported; `new_cycle` follows the policy lists.
    #[default]
    Warn,
    /// As `warn`, and a new cycle always fails, whatever the policy lists say.
    Forbid,
}

// ---- Raw (as written) ----------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    #[serde(default)]
    architecture: Option<RawArchitecture>,
    #[serde(default)]
    critical: Vec<String>,
    #[serde(default)]
    generated: Vec<String>,
    #[serde(default)]
    tests: Option<RawTests>,
    #[serde(default)]
    policy: Option<RawPolicy>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArchitecture {
    #[serde(default)]
    layers: Vec<RawLayer>,
    #[serde(default)]
    rules: Vec<RawRule>,
    #[serde(default)]
    cycles: CycleMode,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayer {
    name: String,
    #[serde(rename = "match")]
    patterns: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    from: String,
    #[serde(default)]
    deny: Option<Vec<String>>,
    #[serde(default)]
    allow: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTests {
    #[serde(default)]
    mode: Option<RawMode>,
}

/// Config spelling is snake_case; the report's `SelectionMode` serialises in SCREAMING_SNAKE_CASE.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawMode {
    Conservative,
    Balanced,
    FastFeedback,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    #[serde(default)]
    fail_on: Vec<Gate>,
    #[serde(default)]
    warn_on: Vec<Gate>,
}

// ---- Validated -----------------------------------------------------------------------------------

/// Repository-relative path globs. `*` and `?` never cross `/`; `**` spans directories.
#[derive(Clone, Debug)]
pub struct PathMatcher {
    patterns: Vec<String>,
    set: GlobSet,
}

impl PathMatcher {
    fn new(patterns: &[String], context: &str) -> Result<Self, ConfigError> {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(compile(pattern, context)?);
        }
        let set = builder.build().map_err(|e| ConfigError::Invalid(format!("{context}: {e}")))?;
        Ok(Self { patterns: patterns.to_vec(), set })
    }

    pub fn empty() -> Self {
        Self { patterns: Vec::new(), set: GlobSet::empty() }
    }

    pub fn is_match(&self, path: &str) -> bool {
        self.set.is_match(path)
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

fn compile(pattern: &str, context: &str) -> Result<Glob, ConfigError> {
    if pattern.trim().is_empty() {
        return Err(ConfigError::Invalid(format!("{context}: empty glob pattern")));
    }
    if pattern.starts_with('/') {
        // Paths are matched repository-relative without a leading slash; such a pattern would
        // silently match nothing.
        return Err(ConfigError::Invalid(format!(
            "{context}: pattern {pattern:?} starts with '/'; patterns are relative to the repository root"
        )));
    }
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(true)
        .build()
        .map_err(|e| ConfigError::Invalid(format!("{context}: invalid glob {pattern:?}: {e}")))
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub matcher: PathMatcher,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuleKind {
    /// `from` must not depend on any listed layer.
    Deny,
    /// `from` may depend only on the listed layers (and itself).
    Allow,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Rule {
    /// Position in `architecture.rules`, zero-based: identifies the rule in findings.
    pub index: usize,
    pub from: String,
    pub kind: RuleKind,
    /// Sorted.
    pub layers: Vec<String>,
}

impl Rule {
    /// True when a dependency `self.from → to` breaks this rule.
    pub fn forbids(&self, to: &str) -> bool {
        let listed = self.layers.iter().any(|l| l == to);
        match self.kind {
            RuleKind::Deny => listed,
            RuleKind::Allow => !listed,
        }
    }

    pub fn describe(&self) -> String {
        match self.kind {
            RuleKind::Deny => format!("{} must not depend on {}", self.from, self.layers.join(", ")),
            RuleKind::Allow if self.layers.is_empty() => format!("{} must not depend on other layers", self.from),
            RuleKind::Allow => format!("{} may depend only on {}", self.from, self.layers.join(", ")),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ArchitectureConfig {
    /// In declaration order: the first layer whose pattern matches a path owns it.
    pub layers: Vec<Layer>,
    pub rules: Vec<Rule>,
    pub cycles: CycleMode,
}

impl ArchitectureConfig {
    pub fn layer_of(&self, path: &str) -> Option<&str> {
        self.layers.iter().find(|l| l.matcher.is_match(path)).map(|l| l.name.as_str())
    }
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub fail_on: BTreeSet<Gate>,
    pub warn_on: BTreeSet<Gate>,
}

impl Default for Policy {
    fn default() -> Self {
        Self { fail_on: BTreeSet::new(), warn_on: Gate::DEFAULT_WARN.into_iter().collect() }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub architecture: ArchitectureConfig,
    pub critical: PathMatcher,
    pub generated: PathMatcher,
    pub tests_mode: Option<SelectionMode>,
    pub policy: Policy,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            architecture: ArchitectureConfig::default(),
            critical: PathMatcher::empty(),
            generated: PathMatcher::empty(),
            tests_mode: None,
            policy: Policy::default(),
        }
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Parses and validates a configuration file's text.
pub fn parse(text: &str) -> Result<Config, ConfigError> {
    if text.len() as u64 > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge { size: text.len() as u64 });
    }
    let raw: RawConfig = serde_saphyr::from_str(text).map_err(|e| ConfigError::Syntax(e.to_string()))?;
    let invalid = |message: String| Err(ConfigError::Invalid(message));
    if raw.version != 1 {
        return invalid(format!("unsupported version {}; this Ripplepath understands version 1", raw.version));
    }

    let raw_arch =
        raw.architecture.unwrap_or(RawArchitecture { layers: Vec::new(), rules: Vec::new(), cycles: CycleMode::Warn });
    if raw_arch.layers.len() > MAX_LAYERS {
        return invalid(format!("{} layers declared; the limit is {MAX_LAYERS}", raw_arch.layers.len()));
    }
    if raw_arch.rules.len() > MAX_RULES {
        return invalid(format!("{} rules declared; the limit is {MAX_RULES}", raw_arch.rules.len()));
    }
    let pattern_count =
        raw_arch.layers.iter().map(|l| l.patterns.len()).sum::<usize>() + raw.critical.len() + raw.generated.len();
    if pattern_count > MAX_PATTERNS {
        return invalid(format!("{pattern_count} path patterns declared; the limit is {MAX_PATTERNS}"));
    }

    let mut names = BTreeSet::new();
    let mut layers = Vec::with_capacity(raw_arch.layers.len());
    for layer in &raw_arch.layers {
        if !valid_name(&layer.name) {
            return invalid(format!(
                "layer name {:?} must be 1-{MAX_NAME_LEN} characters of letters, digits, '_' or '-'",
                layer.name
            ));
        }
        if !names.insert(layer.name.clone()) {
            return invalid(format!("layer {:?} is declared more than once", layer.name));
        }
        if layer.patterns.is_empty() {
            return invalid(format!("layer {:?} has no `match` patterns", layer.name));
        }
        let matcher = PathMatcher::new(&layer.patterns, &format!("layer {:?}", layer.name))?;
        layers.push(Layer { name: layer.name.clone(), matcher });
    }

    let known = |name: &str, context: &str| -> Result<(), ConfigError> {
        if names.contains(name) {
            Ok(())
        } else {
            Err(ConfigError::Invalid(format!("{context} refers to unknown layer {name:?}")))
        }
    };
    let mut rules = Vec::with_capacity(raw_arch.rules.len());
    for (index, rule) in raw_arch.rules.iter().enumerate() {
        let context = format!("rule {} (from {:?})", index + 1, rule.from);
        known(&rule.from, &context)?;
        let (kind, listed) = match (&rule.deny, &rule.allow) {
            (Some(deny), None) => (RuleKind::Deny, deny),
            (None, Some(allow)) => (RuleKind::Allow, allow),
            (Some(_), Some(_)) => return invalid(format!("{context} has both `deny` and `allow`; use one")),
            (None, None) => return invalid(format!("{context} needs `deny` or `allow`")),
        };
        if kind == RuleKind::Deny && listed.is_empty() {
            return invalid(format!("{context}: `deny` lists no layers, so the rule has no effect"));
        }
        let mut layers = BTreeSet::new();
        for name in listed {
            known(name, &context)?;
            if *name == rule.from {
                // Dependencies inside one layer are never checked, so this would be a no-op that
                // looks like a constraint.
                return invalid(format!("{context} lists its own layer; dependencies within a layer are not checked"));
            }
            layers.insert(name.clone());
        }
        rules.push(Rule { index, from: rule.from.clone(), kind, layers: layers.into_iter().collect() });
    }

    let critical = PathMatcher::new(&raw.critical, "critical")?;
    let generated = PathMatcher::new(&raw.generated, "generated")?;

    let policy = match raw.policy {
        None => Policy::default(),
        Some(policy) => {
            let fail_on: BTreeSet<Gate> = policy.fail_on.into_iter().collect();
            let warn_on: BTreeSet<Gate> = policy.warn_on.into_iter().collect();
            if fail_on.contains(&Gate::ConfigInvalid) || warn_on.contains(&Gate::ConfigInvalid) {
                return invalid("policy: `config_invalid` always fails and cannot be listed".to_owned());
            }
            if let Some(both) = fail_on.intersection(&warn_on).next() {
                return invalid(format!("policy: gate `{}` is in both fail_on and warn_on", both.name()));
            }
            Policy { fail_on, warn_on }
        }
    };

    Ok(Config {
        architecture: ArchitectureConfig { layers, rules, cycles: raw_arch.cycles },
        critical,
        generated,
        tests_mode: raw.tests.and_then(|t| t.mode).map(|m| match m {
            RawMode::Conservative => SelectionMode::Conservative,
            RawMode::Balanced => SelectionMode::Balanced,
            RawMode::FastFeedback => SelectionMode::FastFeedback,
        }),
        policy,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    const FULL: &str = r#"
version: 1
architecture:
  layers:
    - name: api
      match: ["src/main/java/com/acme/api/**"]
    - name: domain
      match: ["src/main/java/com/acme/domain/**"]
  rules:
    - from: domain
      deny: [api]
    - from: api
      allow: [domain]
  cycles: forbid
critical: ["src/main/java/com/acme/domain/**"]
generated: ["**/generated/**"]
tests:
  mode: fast_feedback
policy:
  fail_on: [new_architecture_violation, new_cycle]
  warn_on: [breaking_api_change]
"#;

    fn error(text: &str) -> String {
        parse(text).unwrap_err().to_string()
    }

    #[test]
    fn parses_the_full_schema() {
        let config = parse(FULL).unwrap();
        let arch = &config.architecture;
        assert_eq!(arch.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["api", "domain"]);
        assert_eq!(arch.cycles, CycleMode::Forbid);
        assert_eq!(arch.rules[0].describe(), "domain must not depend on api");
        assert_eq!(arch.rules[1].kind, RuleKind::Allow);
        assert!(arch.rules[1].forbids("persistence") && !arch.rules[1].forbids("domain"));
        assert_eq!(arch.layer_of("src/main/java/com/acme/api/A.java"), Some("api"));
        assert_eq!(arch.layer_of("src/main/java/com/acme/other/A.java"), None);
        assert!(config.critical.is_match("src/main/java/com/acme/domain/x/Y.java"));
        assert!(config.generated.is_match("a/generated/B.java"));
        assert_eq!(config.tests_mode, Some(SelectionMode::FastFeedback));
        assert!(config.policy.fail_on.contains(&Gate::NewCycle));
        assert_eq!(config.policy.warn_on.len(), 1);
    }

    #[test]
    fn minimal_config_uses_defaults() {
        let config = parse("version: 1\n").unwrap();
        assert!(config.architecture.layers.is_empty());
        assert_eq!(config.architecture.cycles, CycleMode::Warn);
        assert!(config.policy.fail_on.is_empty());
        assert_eq!(config.policy.warn_on.len(), Gate::DEFAULT_WARN.len());
    }

    #[test]
    fn single_star_does_not_cross_directories() {
        let config = parse("version: 1\ncritical: [\"src/*.ts\"]\n").unwrap();
        assert!(config.critical.is_match("src/a.ts"));
        assert!(!config.critical.is_match("src/x/a.ts"));
    }

    #[test]
    fn misspelled_keys_are_errors() {
        assert!(error("version: 1\npolicy:\n  fail_one: [new_cycle]\n").contains("fail_one"));
        assert!(error("version: 1\narchitecture:\n  layer: []\n").contains("layer"));
        assert!(error("version: 1\ncritcal: []\n").contains("critcal"));
        assert!(error("version: 1\npolicy:\n  fail_on: [new_cycles]\n").contains("new_cycles"));
        assert!(error("version: 1\ntests:\n  mode: fast\n").contains("fast"));
    }

    #[test]
    fn semantic_errors_are_explained() {
        let layer = "architecture:\n  layers:\n    - name: a\n      match: [\"a/**\"]\n";
        let cases = [
            ("version: 2\n".to_owned(), "unsupported version 2"),
            (format!("version: 1\n{layer}    - name: a\n      match: [\"b/**\"]\n"), "more than once"),
            (format!("version: 1\n{layer}  rules:\n    - from: a\n      deny: [b]\n"), "unknown layer \"b\""),
            (format!("version: 1\n{layer}  rules:\n    - from: x\n      deny: [a]\n"), "unknown layer \"x\""),
            (format!("version: 1\n{layer}  rules:\n    - from: a\n      deny: [a]\n"), "its own layer"),
            (format!("version: 1\n{layer}  rules:\n    - from: a\n"), "needs `deny` or `allow`"),
            (format!("version: 1\n{layer}  rules:\n    - from: a\n      deny: []\n"), "no effect"),
            ("version: 1\narchitecture:\n  layers:\n    - name: a\n      match: []\n".to_owned(), "no `match`"),
            ("version: 1\narchitecture:\n  layers:\n    - name: a\n      match: [\"\"]\n".to_owned(), "empty glob"),
            ("version: 1\ncritical: [\"/src/**\"]\n".to_owned(), "starts with '/'"),
            ("version: 1\ncritical: [\"src/[a\"]\n".to_owned(), "invalid glob"),
            (
                "version: 1\narchitecture:\n  layers:\n    - name: \"a b\"\n      match: [\"a\"]\n".to_owned(),
                "layer name",
            ),
            (
                "version: 1\npolicy:\n  fail_on: [new_cycle]\n  warn_on: [new_cycle]\n".to_owned(),
                "both fail_on and warn_on",
            ),
            ("version: 1\npolicy:\n  warn_on: [config_invalid]\n".to_owned(), "cannot be listed"),
        ];
        for (text, expected) in cases {
            let message = error(&text);
            assert!(message.contains(expected), "{text:?}: {message}");
        }
    }

    #[test]
    fn rejects_oversized_input() {
        let text = format!("version: 1\n#{}\n", "x".repeat(MAX_CONFIG_BYTES as usize));
        assert!(matches!(parse(&text), Err(ConfigError::TooLarge { .. })));
    }
}
