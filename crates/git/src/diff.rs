use imara_diff::{Algorithm, Diff, InternedInput};

/// A changed region, 1-based. A zero `*_len` means a pure insertion/removal; `*_start` is then the
/// line *after which* the change happens on that side, matching unified-diff conventions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LineHunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
}

impl LineHunk {
    pub fn old_end(&self) -> u32 {
        self.old_start + self.old_len.saturating_sub(1)
    }

    pub fn new_end(&self) -> u32 {
        self.new_start + self.new_len.saturating_sub(1)
    }
}

/// Line diff using the histogram algorithm (git's own default for `diff.algorithm=histogram`),
/// post-processed with the line slider heuristic so hunk boundaries land where a human expects —
/// which matters because hunks are mapped onto the symbols that contain them.
pub fn line_hunks(before: &str, after: &str) -> Vec<LineHunk> {
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    diff.hunks()
        .map(|hunk| {
            let old_len = hunk.before.end - hunk.before.start;
            let new_len = hunk.after.end - hunk.after.start;
            LineHunk {
                old_start: if old_len == 0 { hunk.before.start } else { hunk.before.start + 1 },
                old_len,
                new_start: if new_len == 0 { hunk.after.start } else { hunk.after.start + 1 },
                new_len,
            }
        })
        .collect()
}

/// Share of lines common to both sides, in percent (Dice coefficient over lines).
///
/// Used for rename detection. Git uses a byte-chunk similarity index; line-based Dice is simpler,
/// explainable in a report, and agrees with git for the usual "moved and lightly edited" case.
pub fn line_similarity(before: &str, after: &str) -> u8 {
    let input = InternedInput::new(before, after);
    let total = input.before.len() + input.after.len();
    if total == 0 {
        return 100;
    }
    let diff = Diff::compute(Algorithm::Histogram, &input);
    let common = input.before.len() - diff.count_removals() as usize;
    ((2 * common * 100) / total) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modification_is_one_hunk_with_1_based_lines() {
        let hunks = line_hunks("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(hunks, vec![LineHunk { old_start: 2, old_len: 1, new_start: 2, new_len: 1 }]);
    }

    #[test]
    fn pure_insertion_and_removal() {
        let inserted = line_hunks("a\nc\n", "a\nb\nc\n");
        assert_eq!(inserted, vec![LineHunk { old_start: 1, old_len: 0, new_start: 2, new_len: 1 }]);
        let removed = line_hunks("a\nb\nc\n", "a\nc\n");
        assert_eq!(removed, vec![LineHunk { old_start: 2, old_len: 1, new_start: 1, new_len: 0 }]);
    }

    #[test]
    fn identical_text_has_no_hunks_and_full_similarity() {
        assert!(line_hunks("x\ny\n", "x\ny\n").is_empty());
        assert_eq!(line_similarity("x\ny\n", "x\ny\n"), 100);
    }

    #[test]
    fn similarity_is_partial_for_light_edits() {
        let before = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let after = "1\n2\n3\n4\n5\n6\n7\n8\n9\nten\n";
        assert_eq!(line_similarity(before, after), 90);
        assert_eq!(line_similarity("a\n", "b\n"), 0);
    }
}
