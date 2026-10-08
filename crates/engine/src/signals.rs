//! High-impact non-code files, recognised by path. These cannot be traced through the symbol graph,
//! so they widen test selection and feed risk signals instead.

use crate::report::FileCategory;

pub fn classify(path: &str) -> Option<FileCategory> {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let in_dir = |dir: &str| lower.split('/').any(|c| c == dir);

    let lockfiles = [
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "bun.lockb",
        "cargo.lock",
        "gradle.lockfile",
        "poetry.lock",
        "pipfile.lock",
        "composer.lock",
        "go.sum",
    ];
    if lockfiles.contains(&name) {
        return Some(FileCategory::Lockfile);
    }
    if lower.contains("db/migration")
        || in_dir("migrations")
        || in_dir("liquibase")
        || in_dir("flyway")
        || name == "schema.prisma"
        || (name.contains("changelog") && (lower.contains("/db/") || in_dir("liquibase")))
    {
        return Some(FileCategory::Migration);
    }
    if lower.starts_with(".github/workflows/")
        || name == ".gitlab-ci.yml"
        || name == "jenkinsfile"
        || lower.starts_with(".circleci/")
    {
        return Some(FileCategory::Ci);
    }
    if name == "dockerfile"
        || name.starts_with("dockerfile.")
        || name.ends_with(".dockerfile")
        || name.starts_with("docker-compose")
        || name.starts_with("compose.")
    {
        return Some(FileCategory::Container);
    }
    let build = [
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
        "gradle.properties",
        "package.json",
        "cargo.toml",
        "makefile",
        "build.xml",
        "go.mod",
        "pyproject.toml",
        "setup.py",
    ];
    if build.contains(&name)
        || (name.starts_with("tsconfig") && name.ends_with(".json"))
        || ["vite.config.", "webpack.config.", "rollup.config.", "babel.config.", "jest.config.", "vitest.config."]
            .iter()
            .any(|p| name.starts_with(p))
    {
        return Some(FileCategory::Build);
    }
    if name.starts_with("application")
        && (name.ends_with(".yml") || name.ends_with(".yaml") || name.ends_with(".properties"))
        || name == ".env"
        || name.starts_with(".env.")
        || in_dir("config") && [".yml", ".yaml", ".json", ".properties", ".toml"].iter().any(|e| name.ends_with(e))
    {
        return Some(FileCategory::Config);
    }
    // After config, so `application.yml` under resources stays CONFIG. Documentation never runs.
    if in_dir("resources") && !name.ends_with(".md") {
        return Some(FileCategory::Resource);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_categories() {
        let cases = [
            ("src/main/resources/db/migration/V2__x.sql", Some(FileCategory::Migration)),
            ("prisma/schema.prisma", Some(FileCategory::Migration)),
            ("app/migrations/0001_init.py", Some(FileCategory::Migration)),
            ("web/package-lock.json", Some(FileCategory::Lockfile)),
            ("Cargo.lock", Some(FileCategory::Lockfile)),
            ("pom.xml", Some(FileCategory::Build)),
            ("web/tsconfig.app.json", Some(FileCategory::Build)),
            ("web/vite.config.ts", Some(FileCategory::Build)),
            (".github/workflows/ci.yml", Some(FileCategory::Ci)),
            ("Dockerfile", Some(FileCategory::Container)),
            ("docker-compose.yml", Some(FileCategory::Container)),
            ("src/main/resources/application-prod.yml", Some(FileCategory::Config)),
            ("src/main/resources/tax-rates.properties", Some(FileCategory::Resource)),
            ("src/test/resources/fixtures/order.json", Some(FileCategory::Resource)),
            ("src/main/resources/README.md", None),
            ("src/main/java/A.java", None),
            ("README.md", None),
        ];
        for (path, expected) in cases {
            assert_eq!(classify(path), expected, "{path}");
        }
    }
}
