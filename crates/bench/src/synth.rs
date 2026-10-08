//! Deterministic synthetic Java + TypeScript repositories.
//!
//! The shape imitates a layered service codebase: packages (Java) and directories (TypeScript) of
//! `package_size` files each, alternating languages. Every package has one interface (`Port`), two
//! implementations, services that hold dependencies as fields and call them, and unit tests for
//! some services. Services depend on earlier units only — mostly in their own package, sometimes in
//! earlier packages of the same language — so dependency chains are deep, like real layering, and
//! impact traversal has real work to do. Calls go through constructs the frontends resolve exactly
//! (typed fields, `this.` members, imports, `implements`), so the graph is dense with edges rather
//! than unresolved references.
//!
//! Output depends only on [`SynthParams`]: the generator uses its own fixed PRNG ([`SplitMix64`])
//! and never reads time, the environment or hash-map order.

use std::fmt::Write as _;

use serde::Serialize;

use crate::rng::SplitMix64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SynthParams {
    /// Source files to generate; rounded up to a whole number of packages.
    pub files: usize,
    pub seed: u64,
    pub package_size: usize,
    /// Dependencies per service (fewer when the candidate pool is smaller).
    pub fan_out: usize,
    /// Percent of a service's dependencies chosen from earlier packages rather than its own.
    pub cross_package_percent: u64,
}

impl SynthParams {
    pub fn new(files: usize, seed: u64) -> Self {
        Self { files, seed, package_size: 20, fan_out: 3, cross_package_percent: 30 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Lang {
    Java,
    Ts,
}

#[derive(Clone, Debug)]
enum Role {
    Port,
    Impl { variant: usize },
    Service { deps: Vec<usize> },
    Test { subject: usize },
}

#[derive(Clone, Debug)]
struct Unit {
    lang: Lang,
    package: usize,
    slot: usize,
    role: Role,
    /// Edit generation: 0 for the original content; each edit sets it to the edit round, which adds
    /// a statement to `compute` (a body change that alters its fingerprint and impacts callers).
    revision: u32,
}

/// The model of a generated repository; files are rendered from it on demand.
#[derive(Clone, Debug)]
pub struct SynthRepo {
    pub params: SynthParams,
    units: Vec<Unit>,
}

/// Slots 0..=2 are the port and its implementations; the last fifth of each package are tests.
const IMPL_SLOTS: usize = 2;

impl SynthRepo {
    pub fn generate(params: SynthParams) -> Self {
        let size = params.package_size.max(IMPL_SLOTS + 3);
        let packages = params.files.div_ceil(size).max(1);
        let tests_per_package = (size / 5).max(1);
        let first_test = size - tests_per_package;
        let mut rng = SplitMix64::new(params.seed);
        let mut units: Vec<Unit> = Vec::with_capacity(packages * size);
        for package in 0..packages {
            let lang = if package % 2 == 0 { Lang::Java } else { Lang::Ts };
            let base = units.len();
            for slot in 0..size {
                let role = if slot == 0 {
                    Role::Port
                } else if slot <= IMPL_SLOTS {
                    Role::Impl { variant: slot }
                } else if slot < first_test {
                    let deps = choose_deps(&mut rng, &params, package, slot, first_test, lang, size);
                    Role::Service { deps }
                } else {
                    // Tests cover services spread over the package.
                    let services = first_test - (IMPL_SLOTS + 1);
                    let k = slot - first_test;
                    let subject_slot = IMPL_SLOTS + 1 + k * services / tests_per_package;
                    Role::Test { subject: base + subject_slot }
                };
                units.push(Unit { lang, package, slot, role, revision: 0 });
            }
        }
        Self { params, units }
    }

    pub fn source_files(&self) -> usize {
        self.units.len()
    }

    /// Every file of the current revision, sorted by path.
    pub fn render_all(&self) -> Vec<(String, String)> {
        let mut files: Vec<(String, String)> = (0..self.units.len()).map(|i| self.render(i)).collect();
        files.push(("README.md".to_owned(), "# Synthetic Ripplepath benchmark repository\n".to_owned()));
        files.push((
            "package.json".to_owned(),
            "{\n  \"name\": \"synth\",\n  \"private\": true,\n  \"devDependencies\": { \"vitest\": \"*\" }\n}\n"
                .to_owned(),
        ));
        files.push((
            "pom.xml".to_owned(),
            "<project><modelVersion>4.0.0</modelVersion><groupId>com.synth</groupId>\
             <artifactId>synth</artifactId><version>1</version></project>\n"
                .to_owned(),
        ));
        files.sort();
        files
    }

    /// Edits `count` distinct services (fewer if there are not that many), chosen deterministically
    /// from the seed and `round`, and returns the rewritten files sorted by path.
    pub fn edit(&mut self, count: usize, round: u32) -> Vec<(String, String)> {
        let mut candidates: Vec<usize> =
            (0..self.units.len()).filter(|&i| matches!(self.units[i].role, Role::Service { .. })).collect();
        let mut rng = SplitMix64::new(self.params.seed ^ u64::from(round).wrapping_mul(0xA24B_AED4_963E_E407));
        let take = count.min(candidates.len());
        // Partial Fisher–Yates: the first `take` entries become a uniform sample without repeats.
        for i in 0..take {
            let j = i + rng.below(candidates.len() - i);
            candidates.swap(i, j);
        }
        let mut edited: Vec<(String, String)> = candidates[..take]
            .iter()
            .map(|&i| {
                self.units[i].revision = round;
                self.render(i)
            })
            .collect();
        edited.sort();
        edited
    }

    fn name(&self, index: usize) -> String {
        let unit = &self.units[index];
        let p = unit.package;
        match &unit.role {
            Role::Port => format!("Port{p}"),
            Role::Impl { variant } => format!("Port{p}Impl{variant}"),
            Role::Service { .. } => format!("Svc{p}x{}", unit.slot),
            Role::Test { subject } => format!("{}Test", self.name(*subject)),
        }
    }

    fn java_package(package: usize) -> String {
        format!("com.synth.p{package:04}")
    }

    fn path(&self, index: usize) -> String {
        let unit = &self.units[index];
        let p = unit.package;
        match unit.lang {
            Lang::Java => {
                let root = if matches!(unit.role, Role::Test { .. }) { "src/test/java" } else { "src/main/java" };
                format!("{root}/com/synth/p{p:04}/{}.java", self.name(index))
            }
            Lang::Ts => format!("web/src/p{p:04}/{}", self.ts_file(index)),
        }
    }

    fn ts_file(&self, index: usize) -> String {
        let unit = &self.units[index];
        match &unit.role {
            Role::Port => "port.ts".to_owned(),
            Role::Impl { variant } => format!("port-impl{variant}.ts"),
            Role::Service { .. } => format!("svc{}.ts", unit.slot),
            Role::Test { subject } => format!("svc{}.test.ts", self.units[*subject].slot),
        }
    }

    fn render(&self, index: usize) -> (String, String) {
        let content = match self.units[index].lang {
            Lang::Java => self.render_java(index),
            Lang::Ts => self.render_ts(index),
        };
        (self.path(index), content)
    }

    fn render_java(&self, index: usize) -> String {
        let unit = &self.units[index];
        let name = self.name(index);
        let mut out = format!("package {};\n\n", Self::java_package(unit.package));
        match &unit.role {
            Role::Port => {
                let _ = write!(
                    out,
                    "/** Port of package {p}. */\npublic interface {name} {{\n    int compute(int x);\n\n    int handle(int x);\n}}\n",
                    p = unit.package
                );
            }
            Role::Impl { variant } => {
                let port = self.name(index - variant);
                let _ = write!(
                    out,
                    "public class {name} implements {port} {{\n    private int calls;\n\n    @Override\n    public int compute(int x) {{\n        calls++;\n{bump}        return x + {variant};\n    }}\n\n    @Override\n    public int handle(int x) {{\n        return compute(x) - calls;\n    }}\n}}\n",
                    bump = java_bump(unit.revision),
                );
            }
            Role::Service { deps } => {
                let mut imports: Vec<String> = deps
                    .iter()
                    .filter(|&&d| self.units[d].package != unit.package)
                    .map(|&d| format!("import {}.{};\n", Self::java_package(self.units[d].package), self.name(d)))
                    .collect();
                imports.sort();
                imports.dedup();
                for import in &imports {
                    out.push_str(import);
                }
                if !imports.is_empty() {
                    out.push('\n');
                }
                let _ = writeln!(out, "public class {name} {{");
                for (k, &d) in deps.iter().enumerate() {
                    let _ = writeln!(out, "    private final {} dep{k};", self.name(d));
                }
                let params: Vec<String> =
                    deps.iter().enumerate().map(|(k, &d)| format!("{} dep{k}", self.name(d))).collect();
                let _ = writeln!(out, "\n    public {name}({}) {{", params.join(", "));
                for k in 0..deps.len() {
                    let _ = writeln!(out, "        this.dep{k} = dep{k};");
                }
                out.push_str("    }\n\n    public int compute(int x) {\n        int acc = x;\n");
                for k in 0..deps.len() {
                    let _ = writeln!(out, "        acc += dep{k}.compute(acc);");
                }
                out.push_str(&java_bump(unit.revision));
                out.push_str("        return helper0(acc);\n    }\n\n");
                out.push_str("    public int handle(int x) {\n        if (x < 0) {\n            return 0;\n        }\n        return compute(x) * 2;\n    }\n");
                let helpers = helper_count(unit.slot);
                for h in 0..helpers {
                    let body = if h + 1 < helpers {
                        format!("helper{}(v) + {h}", h + 1)
                    } else {
                        format!("v + {}", unit.slot)
                    };
                    let _ = write!(out, "\n    private int helper{h}(int v) {{\n        return {body};\n    }}\n");
                }
                out.push_str("}\n");
            }
            Role::Test { subject } => {
                let subject_name = self.name(*subject);
                let arity = self.deps_of(*subject).len();
                let args = vec!["null"; arity].join(", ");
                let _ = write!(
                    out,
                    "import static org.junit.jupiter.api.Assertions.assertEquals;\n\nimport org.junit.jupiter.api.Test;\n\nclass {name} {{\n    @Test\n    void handlesNegativeInput() {{\n        {subject_name} subject = new {subject_name}({args});\n        assertEquals(0, subject.handle(-1));\n    }}\n\n    @Test\n    void computes() {{\n        {subject_name} subject = new {subject_name}({args});\n        assertEquals(1, subject.compute(1));\n    }}\n}}\n"
                );
            }
        }
        out
    }

    fn render_ts(&self, index: usize) -> String {
        let unit = &self.units[index];
        let name = self.name(index);
        let mut out = String::new();
        match &unit.role {
            Role::Port => {
                let _ = write!(
                    out,
                    "/** Port of package {p}. */\nexport interface {name} {{\n  compute(x: number): number;\n  handle(x: number): number;\n}}\n",
                    p = unit.package
                );
            }
            Role::Impl { variant } => {
                let port = self.name(index - variant);
                let _ = write!(
                    out,
                    "import type {{ {port} }} from \"./port\";\n\nexport class {name} implements {port} {{\n  private calls = 0;\n\n  compute(x: number): number {{\n    this.calls++;\n{bump}    return x + {variant};\n  }}\n\n  handle(x: number): number {{\n    return this.compute(x) - this.calls;\n  }}\n}}\n",
                    bump = ts_bump(unit.revision),
                );
            }
            Role::Service { deps } => {
                let mut imports: Vec<String> = deps
                    .iter()
                    .map(|&d| {
                        let dep = &self.units[d];
                        let file = self.ts_file(d);
                        let module = file.trim_end_matches(".ts");
                        let from = if dep.package == unit.package {
                            format!("./{module}")
                        } else {
                            format!("../p{:04}/{module}", dep.package)
                        };
                        let kind = if matches!(dep.role, Role::Port) { "import type" } else { "import" };
                        format!("{kind} {{ {} }} from \"{from}\";\n", self.name(d))
                    })
                    .collect();
                imports.sort();
                imports.dedup();
                for import in &imports {
                    out.push_str(import);
                }
                let _ = writeln!(out, "\nexport class {name} {{\n  constructor(");
                for (k, &d) in deps.iter().enumerate() {
                    let _ = writeln!(out, "    private readonly dep{k}: {},", self.name(d));
                }
                out.push_str("  ) {}\n\n  compute(x: number): number {\n    let acc = x;\n");
                for k in 0..deps.len() {
                    let _ = writeln!(out, "    acc += this.dep{k}.compute(acc);");
                }
                out.push_str(&ts_bump(unit.revision));
                out.push_str("    return this.helper0(acc);\n  }\n\n");
                out.push_str("  handle(x: number): number {\n    if (x < 0) {\n      return 0;\n    }\n    return this.compute(x) * 2;\n  }\n");
                let helpers = helper_count(unit.slot);
                for h in 0..helpers {
                    let body = if h + 1 < helpers {
                        format!("this.helper{}(v) + {h}", h + 1)
                    } else {
                        format!("v + {}", unit.slot)
                    };
                    let _ = write!(out, "\n  private helper{h}(v: number): number {{\n    return {body};\n  }}\n");
                }
                out.push_str("}\n");
            }
            Role::Test { subject } => {
                let subject_name = self.name(*subject);
                let module = self.ts_file(*subject);
                let module = module.trim_end_matches(".ts");
                let args = vec!["null as never"; self.deps_of(*subject).len()].join(", ");
                let _ = write!(
                    out,
                    "import {{ describe, expect, it }} from \"vitest\";\nimport {{ {subject_name} }} from \"./{module}\";\n\ndescribe(\"{subject_name}\", () => {{\n  it(\"handles negative input\", () => {{\n    const subject = new {subject_name}({args});\n    expect(subject.handle(-1)).toBe(0);\n  }});\n\n  it(\"computes\", () => {{\n    const subject = new {subject_name}({args});\n    expect(subject.compute(1)).toBe(1);\n  }});\n}});\n"
                );
            }
        }
        out
    }

    fn deps_of(&self, index: usize) -> &[usize] {
        match &self.units[index].role {
            Role::Service { deps } => deps,
            _ => &[],
        }
    }
}

fn helper_count(slot: usize) -> usize {
    2 + slot % 3
}

fn java_bump(revision: u32) -> String {
    if revision == 0 { String::new() } else { format!("        x += {revision};\n") }
}

fn ts_bump(revision: u32) -> String {
    if revision == 0 { String::new() } else { format!("    x += {revision};\n") }
}

/// Dependencies of the service at (`package`, `slot`): earlier non-test units of its own package,
/// or of an earlier package of the same language. Distinct and in choice order.
fn choose_deps(
    rng: &mut SplitMix64,
    params: &SynthParams,
    package: usize,
    slot: usize,
    first_test: usize,
    lang: Lang,
    size: usize,
) -> Vec<usize> {
    // Earlier packages of the same language: they alternate, so same parity.
    let earlier: Vec<usize> = (0..package).filter(|p| (p % 2 == 0) == (lang == Lang::Java)).collect();
    let mut deps: Vec<usize> = Vec::with_capacity(params.fan_out);
    for _ in 0..params.fan_out {
        let candidate = if !earlier.is_empty() && rng.chance(params.cross_package_percent) {
            let p = earlier[rng.below(earlier.len())];
            p * size + rng.below(first_test)
        } else {
            package * size + rng.below(slot)
        };
        if !deps.contains(&candidate) {
            deps.push(candidate);
        }
    }
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_and_seed_dependent() {
        let a = SynthRepo::generate(SynthParams::new(200, 7)).render_all();
        let b = SynthRepo::generate(SynthParams::new(200, 7)).render_all();
        let c = SynthRepo::generate(SynthParams::new(200, 8)).render_all();
        assert_eq!(a, b);
        assert_ne!(a, c);
        // 200 source files plus three project files.
        assert_eq!(a.len(), 203);
        assert!(a.iter().any(|(p, _)| p.ends_with("Test.java")));
        assert!(a.iter().any(|(p, _)| p.ends_with(".test.ts")));
    }

    #[test]
    fn edits_touch_exactly_the_requested_services() {
        let mut repo = SynthRepo::generate(SynthParams::new(200, 7));
        let before: std::collections::BTreeMap<_, _> = repo.render_all().into_iter().collect();
        let edited = repo.edit(10, 1);
        assert_eq!(edited.len(), 10);
        for (path, content) in &edited {
            assert_ne!(before.get(path), Some(content), "{path} must change");
        }
        let after: std::collections::BTreeMap<_, _> = repo.render_all().into_iter().collect();
        let changed = before.iter().filter(|(p, c)| after.get(*p) != Some(*c)).count();
        assert_eq!(changed, 10);
        let mut again = SynthRepo::generate(SynthParams::new(200, 7));
        assert_eq!(again.edit(10, 1), edited);
    }
}
