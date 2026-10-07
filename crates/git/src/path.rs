use std::fmt;

/// Why a path recorded in a Git tree was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathRejection {
    NotUtf8,
    Empty,
    Absolute,
    /// `.` or `..` components, or empty components from doubled separators.
    NonNormalComponent,
    /// A `.git` component. Git itself refuses to check these out; a tree containing one is
    /// almost certainly crafted.
    GitDirectory,
    ControlCharacter,
    Backslash,
}

impl fmt::Display for PathRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::NotUtf8 => "path is not valid UTF-8",
            Self::Empty => "empty path",
            Self::Absolute => "absolute path",
            Self::NonNormalComponent => "path contains '.', '..' or empty components",
            Self::GitDirectory => "path contains a .git component",
            Self::ControlCharacter => "path contains control characters",
            Self::Backslash => "path contains a backslash",
        };
        f.write_str(text)
    }
}

/// Validates a repository-relative path taken from a Git tree.
///
/// Ripplepath never writes these paths to disk in revision mode, but they flow into reports,
/// HTML, SARIF and (in working-tree mode) filesystem reads. Rejecting anything that could escape
/// the repository root or be interpreted differently per platform keeps every downstream consumer
/// simple. Backslashes are refused because on Windows they are separators: `a\..\..\x` would be a
/// traversal there while looking like a single odd filename on Unix.
pub fn validate_repo_path(raw: &[u8]) -> Result<String, PathRejection> {
    let path = std::str::from_utf8(raw).map_err(|_| PathRejection::NotUtf8)?;
    if path.is_empty() {
        return Err(PathRejection::Empty);
    }
    if path.starts_with('/') || has_drive_prefix(path) {
        return Err(PathRejection::Absolute);
    }
    if path.chars().any(char::is_control) {
        return Err(PathRejection::ControlCharacter);
    }
    if path.contains('\\') {
        return Err(PathRejection::Backslash);
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(PathRejection::NonNormalComponent);
        }
        if component.eq_ignore_ascii_case(".git") {
            return Err(PathRejection::GitDirectory);
        }
    }
    Ok(path.to_owned())
}

fn has_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_paths() {
        assert_eq!(validate_repo_path(b"src/main/java/A.java"), Ok("src/main/java/A.java".to_owned()));
        assert_eq!(validate_repo_path(b"weird name (1).ts"), Ok("weird name (1).ts".to_owned()));
        assert_eq!(validate_repo_path("dir/ünïcode.java".as_bytes()), Ok("dir/ünïcode.java".to_owned()));
    }

    #[test]
    fn rejects_malicious_paths() {
        let cases: &[(&[u8], PathRejection)] = &[
            (b"../etc/passwd", PathRejection::NonNormalComponent),
            (b"a/../../b", PathRejection::NonNormalComponent),
            (b"a//b", PathRejection::NonNormalComponent),
            (b"./a", PathRejection::NonNormalComponent),
            (b"/etc/passwd", PathRejection::Absolute),
            (b"C:/Windows/system32", PathRejection::Absolute),
            (b"a\\..\\..\\b", PathRejection::Backslash),
            (b".git/config", PathRejection::GitDirectory),
            (b"sub/.GIT/hooks/pre-commit", PathRejection::GitDirectory),
            (b"a\nb", PathRejection::ControlCharacter),
            (b"a\0b", PathRejection::ControlCharacter),
            (b"\xff\xfe", PathRejection::NotUtf8),
            (b"", PathRejection::Empty),
        ];
        for (input, expected) in cases {
            assert_eq!(validate_repo_path(input), Err(*expected), "{input:?}");
        }
    }
}
