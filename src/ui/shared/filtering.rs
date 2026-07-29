pub fn fzf_style_match(haystack: &str, needle: &str) -> Option<(i32, Vec<usize>)> {
    if needle.is_empty() {
        return Some((0, Vec::new()));
    }

    let haystack_chars: Vec<char> = haystack.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();

    let positions = greedy_subsequence_positions(&haystack_chars, &needle_chars)?;
    let score = compute_fzf_score(&haystack_chars, &needle_chars, &positions);
    Some(best_with_contiguous(
        &haystack_chars,
        &needle_chars,
        score,
        positions,
        compute_fzf_score,
    ))
}

fn compute_fzf_score(haystack: &[char], needle: &[char], positions: &[usize]) -> i32 {
    let mut score = 0i32;
    for (i, &position) in positions.iter().enumerate() {
        if position == 0 {
            score += 12;
        }
        if position > 0 {
            let prev = haystack[position - 1];
            if prev == ' ' || prev == '_' || prev == '-' || prev == '/' || prev == '.' {
                score += 10;
            }
        }
        if i > 0 {
            let prev = positions[i - 1];
            if position == prev + 1 {
                score += 18;
            } else {
                score -= (position - prev - 1) as i32;
            }
        }
        if haystack[position] == needle[i] {
            score += 4;
        }
    }
    score -= (haystack.len() as i32) / 5;
    score
}

fn char_lower(ch: char) -> char {
    ch.to_lowercase().next().unwrap_or(ch)
}

/// Leftmost case-insensitive subsequence positions of `needle` in `haystack`.
fn greedy_subsequence_positions(haystack: &[char], needle: &[char]) -> Option<Vec<usize>> {
    let mut positions = Vec::with_capacity(needle.len());
    let mut hay_idx = 0usize;
    for &needle_ch in needle {
        let needle_lower = char_lower(needle_ch);
        loop {
            if hay_idx >= haystack.len() {
                return None;
            }
            let idx = hay_idx;
            hay_idx += 1;
            if char_lower(haystack[idx]) == needle_lower {
                positions.push(idx);
                break;
            }
        }
    }
    Some(positions)
}

/// The greedy scan anchors on the first occurrence of each needle char, which
/// can scatter the match even when the needle appears verbatim later in the
/// haystack (e.g. "search" in "src/global_search_index.rs" anchors on the
/// leading 's'). Score every contiguous occurrence too and keep whichever
/// alignment scores best, so exact substrings rank as exact matches.
fn best_with_contiguous(
    haystack: &[char],
    needle: &[char],
    greedy_score: i32,
    greedy_positions: Vec<usize>,
    score_fn: impl Fn(&[char], &[char], &[usize]) -> i32,
) -> (i32, Vec<usize>) {
    let mut best = (greedy_score, greedy_positions);
    let Some(last_start) = haystack.len().checked_sub(needle.len()) else {
        return best;
    };
    for start in 0..=last_start {
        let matches = needle
            .iter()
            .zip(&haystack[start..])
            .all(|(&n, &h)| char_lower(n) == char_lower(h));
        if !matches {
            continue;
        }
        let positions: Vec<usize> = (start..start + needle.len()).collect();
        let score = score_fn(haystack, needle, &positions);
        if score > best.0 {
            best = (score, positions);
        }
    }
    best
}

/// Fuzzy match `haystack` against `needle` (case-insensitive).
///
/// First tries strict in-order subsequence matching. If that fails and the needle
/// contains whitespace, falls back to per-token matching where each
/// whitespace-separated token must subsequence-match the haystack but the tokens
/// themselves may appear in any order. The token fallback receives a constant
/// score penalty so true in-order matches always rank above it.
pub fn fuzzy_match(haystack: &str, needle: &str) -> Option<(i32, Vec<usize>)> {
    if needle.is_empty() {
        return Some((0, Vec::new()));
    }
    if let Some(result) = fuzzy_match_strict(haystack, needle) {
        return Some(result);
    }
    fuzzy_match_tokens(haystack, needle)
}

fn fuzzy_match_strict(haystack: &str, needle: &str) -> Option<(i32, Vec<usize>)> {
    let haystack_chars: Vec<char> = haystack.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();

    let positions = greedy_subsequence_positions(&haystack_chars, &needle_chars)?;
    let score = compute_score(&haystack_chars, &needle_chars, &positions);
    Some(best_with_contiguous(
        &haystack_chars,
        &needle_chars,
        score,
        positions,
        compute_score,
    ))
}

const TOKEN_FALLBACK_PENALTY: i32 = 50;

fn fuzzy_match_tokens(haystack: &str, needle: &str) -> Option<(i32, Vec<usize>)> {
    let tokens: Vec<&str> = needle.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }

    let mut total: i32 = 0;
    let mut all_positions: Vec<usize> = Vec::new();
    for token in &tokens {
        let (score, positions) = fuzzy_match_strict(haystack, token)?;
        total = total.saturating_add(score);
        all_positions.extend(positions);
    }
    all_positions.sort_unstable();
    all_positions.dedup();
    total = total.saturating_sub(TOKEN_FALLBACK_PENALTY);
    Some((total, all_positions))
}

fn compute_score(haystack: &[char], needle: &[char], positions: &[usize]) -> i32 {
    let mut score: i32 = 0;

    for (i, &position) in positions.iter().enumerate() {
        if position == 0 {
            score += 8;
        }

        if position > 0 {
            let prev = haystack[position - 1];
            if prev == ' ' || prev == '_' || prev == '-' || prev == '.' || prev == '/' {
                score += 8;
            }
        }

        if i > 0 && position == positions[i - 1] + 1 {
            score += 12;
        }

        if haystack[position] == needle[i] {
            score += 4;
        }

        if i > 0 {
            let gap = position as i32 - positions[i - 1] as i32 - 1;
            score -= gap;
        }
    }

    score -= (haystack.len() as i32) / 4;

    score
}

#[cfg(test)]
mod tests {
    use super::{fuzzy_match, fzf_style_match};

    #[test]
    fn fuzzy_match_is_case_insensitive() {
        let result = fuzzy_match("Save File", "sf");
        assert!(result.is_some());
    }

    #[test]
    fn fuzzy_match_returns_none_for_missing_sequence() {
        let result = fuzzy_match("Save File", "xyz");
        assert!(result.is_none());
    }

    #[test]
    fn fzf_style_match_prefers_consecutive_matches() {
        let (consecutive_score, _) = fzf_style_match("abcdef", "abc").expect("consecutive");
        let (sparse_score, _) = fzf_style_match("axbxcxdef", "abc").expect("sparse");
        assert!(consecutive_score > sparse_score);
    }

    #[test]
    fn fuzzy_match_token_fallback_allows_out_of_order_tokens() {
        // Strict subsequence "github co" cannot match "Copy GitHub URL" because
        // the 'co' in 'Copy' precedes 'github'. The token fallback should match.
        let result = fuzzy_match("Copy GitHub URL", "github co");
        assert!(result.is_some(), "token fallback should match");
    }

    #[test]
    fn fuzzy_match_strict_outranks_token_fallback() {
        // In-order match wins over reordered-token match.
        let (in_order, _) = fuzzy_match("Copy GitHub URL", "co github").expect("strict");
        let (out_of_order, _) = fuzzy_match("Copy GitHub URL", "github co").expect("fallback");
        assert!(in_order > out_of_order);
    }

    #[test]
    fn fuzzy_match_token_fallback_requires_all_tokens() {
        // "xyz" is absent from haystack, so the fallback must still fail.
        let result = fuzzy_match("Copy GitHub URL", "github xyz");
        assert!(result.is_none());
    }

    #[test]
    fn fuzzy_match_single_token_does_not_take_fallback() {
        // Single-token query: no fallback path; absence means None.
        let result = fuzzy_match("Copy GitHub URL", "xyz");
        assert!(result.is_none());
    }

    #[test]
    fn fuzzy_match_exact_substring_outranks_scattered_match() {
        // "modules/search.rs" contains the query verbatim; the greedy scan used
        // to anchor on the leading 's' of "modules" and scatter the rest,
        // letting a non-exact candidate overtake it.
        let (exact, _) = fuzzy_match("modules/search.rs", "search").expect("exact");
        let (scattered, _) = fuzzy_match("sea_rch.rs", "search").expect("scattered");
        assert!(exact > scattered, "exact={exact} scattered={scattered}");
    }

    #[test]
    fn fuzzy_match_prefers_contiguous_occurrence_positions() {
        let (_, positions) = fuzzy_match("src/global_search_index.rs", "search").expect("match");
        assert_eq!(positions, (11..17).collect::<Vec<_>>());
    }

    #[test]
    fn fuzzy_match_exact_equality_ranks_above_superstrings() {
        let (equal, _) = fuzzy_match("search", "search").expect("equal");
        for other in ["sea_rch.rs", "search.rs", "src/search.rs", "s_e_a_r_c_h"] {
            let (score, _) = fuzzy_match(other, "search").expect(other);
            assert!(equal > score, "{other}: {score} >= {equal}");
        }
    }

    #[test]
    fn fzf_style_match_prefers_contiguous_occurrence_positions() {
        let (_, positions) =
            fzf_style_match("src/global_search_index.rs", "search").expect("match");
        assert_eq!(positions, (11..17).collect::<Vec<_>>());
    }
}
