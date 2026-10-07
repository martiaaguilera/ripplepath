use crate::diff::line_similarity;

pub struct RenameCandidate<'a> {
    pub path: &'a str,
    pub blob: &'a str,
    /// `None` for binary/oversized content: such files can only be paired by exact blob id.
    pub text: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RenamePair {
    pub from: String,
    pub to: String,
    pub similarity: u8,
}

/// Git's default rename threshold (`-M50%`).
pub const DEFAULT_THRESHOLD: u8 = 50;

/// Above this many deleted×added comparisons similarity matching is skipped and only exact renames
/// are reported. Pairwise diffing is quadratic; a mass move of thousands of files would otherwise
/// dominate analysis time. The caller reports the skip so the gap is visible.
pub const MAX_SIMILARITY_COMPARISONS: usize = 10_000;

/// Pairs deleted with added files.
///
/// Exact blob matches first, then greedy best-similarity matching. Ties break on path so the result
/// does not depend on input order. Returns the pairs and whether similarity matching was skipped.
pub fn pair_renames(deleted: &[RenameCandidate<'_>], added: &[RenameCandidate<'_>]) -> (Vec<RenamePair>, bool) {
    let mut deleted_used = vec![false; deleted.len()];
    let mut added_used = vec![false; added.len()];
    let mut pairs = Vec::new();

    let mut deleted_order: Vec<usize> = (0..deleted.len()).collect();
    deleted_order.sort_by_key(|&i| deleted[i].path);
    let mut added_order: Vec<usize> = (0..added.len()).collect();
    added_order.sort_by_key(|&i| added[i].path);

    for &d in &deleted_order {
        if let Some(&a) = added_order.iter().find(|&&a| !added_used[a] && added[a].blob == deleted[d].blob) {
            deleted_used[d] = true;
            added_used[a] = true;
            pairs.push(RenamePair { from: deleted[d].path.to_owned(), to: added[a].path.to_owned(), similarity: 100 });
        }
    }

    let remaining_deleted: Vec<usize> = deleted_order.into_iter().filter(|&d| !deleted_used[d]).collect();
    let remaining_added: Vec<usize> = added_order.into_iter().filter(|&a| !added_used[a]).collect();
    let skipped = remaining_deleted.len() * remaining_added.len() > MAX_SIMILARITY_COMPARISONS;

    if !skipped {
        let mut scored = Vec::new();
        for &d in &remaining_deleted {
            for &a in &remaining_added {
                let (Some(before), Some(after)) = (deleted[d].text, added[a].text) else {
                    continue;
                };
                if extension(deleted[d].path) != extension(added[a].path) {
                    continue;
                }
                let similarity = line_similarity(before, after);
                if similarity >= DEFAULT_THRESHOLD {
                    scored.push((std::cmp::Reverse(similarity), deleted[d].path, added[a].path, d, a));
                }
            }
        }
        scored.sort();
        for (std::cmp::Reverse(similarity), from, to, d, a) in scored {
            if deleted_used[d] || added_used[a] {
                continue;
            }
            deleted_used[d] = true;
            added_used[a] = true;
            pairs.push(RenamePair { from: from.to_owned(), to: to.to_owned(), similarity });
        }
    }

    pairs.sort();
    (pairs, skipped)
}

fn extension(path: &str) -> Option<&str> {
    path.rsplit_once('/').map_or(path, |(_, name)| name).rsplit_once('.').map(|(_, ext)| ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate<'a>(path: &'a str, blob: &'a str, text: &'a str) -> RenameCandidate<'a> {
        RenameCandidate { path, blob, text: Some(text) }
    }

    #[test]
    fn exact_blob_match_is_full_similarity() {
        let (pairs, skipped) =
            pair_renames(&[candidate("a/Old.java", "b1", "x\n")], &[candidate("b/New.java", "b1", "x\n")]);
        assert!(!skipped);
        assert_eq!(pairs, vec![RenamePair { from: "a/Old.java".into(), to: "b/New.java".into(), similarity: 100 }]);
    }

    #[test]
    fn similar_content_pairs_and_dissimilar_does_not() {
        let before = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let edited = "1\n2\n3\n4\n5\n6\n7\n8\n9\nten\n";
        let (pairs, _) = pair_renames(
            &[candidate("Old.java", "b1", before), candidate("Gone.java", "b2", "unrelated\n")],
            &[candidate("New.java", "b3", edited), candidate("Other.java", "b4", "different\n")],
        );
        assert_eq!(pairs, vec![RenamePair { from: "Old.java".into(), to: "New.java".into(), similarity: 90 }]);
    }

    #[test]
    fn extension_change_is_not_a_rename_by_similarity() {
        let (pairs, _) = pair_renames(&[candidate("a.java", "b1", "x\ny\n")], &[candidate("a.ts", "b2", "x\ny\n")]);
        assert!(pairs.is_empty());
    }

    #[test]
    fn result_is_independent_of_input_order() {
        let text = "same\ncontent\nhere\n";
        let deleted = [candidate("z.java", "d1", text), candidate("a.java", "d2", text)];
        let added = [candidate("m.java", "a1", text), candidate("b.java", "a2", text)];
        let (forward, _) = pair_renames(&deleted, &added);
        let deleted_rev = [candidate("a.java", "d2", text), candidate("z.java", "d1", text)];
        let added_rev = [candidate("b.java", "a2", text), candidate("m.java", "a1", text)];
        let (reversed, _) = pair_renames(&deleted_rev, &added_rev);
        assert_eq!(forward, reversed);
    }
}
