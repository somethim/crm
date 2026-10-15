//! Per-token matching: exact, prefix and typo-tolerant scores.

/// `q == t`.
pub(crate) const EXACT: u8 = 3;
/// `t` starts with `q`.
pub(crate) const PREFIX: u8 = 2;
/// Within the allowed edits of `q` (whole `t` or a prefix of `t`).
pub(crate) const TYPO: u8 = 1;

/// Allowed edits for a query token of `len` characters (old `num_typos: 2`).
pub(crate) const fn allowed_edits(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4..=6 => 1,
        _ => 2,
    }
}

const BUCKETS: usize = 64;

/// Character histogram over hashed buckets, for a cheap edit-distance lower bound.
type Hist = [u16; BUCKETS];

fn bucket(c: char) -> usize {
    c as usize % BUCKETS
}

/// One folded query token, prepared for matching against index tokens.
#[derive(Debug, Clone)]
pub(crate) struct QueryToken {
    text: String,
    len: usize,
    allowed: usize,
    hist: Hist,
}

impl QueryToken {
    /// `text` must already be folded (one element of `text::tokens`).
    pub(crate) fn new(text: String) -> Self {
        let mut hist = [0u16; BUCKETS];
        let mut len = 0;
        for c in text.chars() {
            hist[bucket(c)] = hist[bucket(c)].saturating_add(1);
            len += 1;
        }
        Self {
            text,
            len,
            allowed: allowed_edits(len),
            hist,
        }
    }

    /// Exact or prefix match only (codes, NIPTs).
    pub(crate) fn score_exact(&self, t: &str) -> u8 {
        if t == self.text {
            EXACT
        } else if t.starts_with(self.text.as_str()) {
            PREFIX
        } else {
            0
        }
    }

    /// Exact, prefix, or else a typo match.
    pub(crate) fn score_fuzzy(&self, t: &str) -> u8 {
        let score = self.score_exact(t);
        if score > 0 || self.allowed == 0 || !self.typo_match(t) {
            score
        } else {
            TYPO
        }
    }

    /// Damerau-Levenshtein of `q` against the prefixes `t[..len(q) - 1]`,
    /// `t[..len(q)]`, `t[..len(q) + 1]` and the whole `t` (lengths in chars,
    /// the whole `t` only within the allowed length difference).
    ///
    /// One pass over `t` keeps a character-bag lower bound on the distance
    /// (every edit changes, by at most one, how many characters one side has
    /// that the other lacks), so `strsim` runs only on plausible slices.
    fn typo_match(&self, t: &str) -> bool {
        let (n, k) = (self.len, self.allowed);
        let stop = n + k.max(1);
        let mut hist = self.hist;
        let (mut len, mut matched, mut extra) = (0usize, 0usize, 0usize);
        let mut chars = t.char_indices().peekable();
        loop {
            let end = chars.peek().map_or(t.len(), |&(byte, _)| byte);
            let whole = end == t.len();
            let candidate = if whole {
                len.abs_diff(n) <= k
            } else {
                len + 1 >= n && len <= n + 1
            };
            if len > 0
                && candidate
                && (n - matched).max(extra) <= k
                && strsim::damerau_levenshtein(&self.text, &t[..end]) <= k
            {
                return true;
            }
            if whole || len >= stop {
                return false;
            }
            let Some((_, c)) = chars.next() else {
                return false;
            };
            let slot = &mut hist[bucket(c)];
            if *slot > 0 {
                *slot -= 1;
                matched += 1;
            } else {
                extra += 1;
                if extra > k {
                    // Longer slices keep at least this many extra characters.
                    return false;
                }
            }
            len += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> QueryToken {
        QueryToken::new(s.to_owned())
    }

    #[test]
    fn allowed_edits_by_length() {
        assert_eq!(allowed_edits(3), 0);
        assert_eq!(allowed_edits(4), 1);
        assert_eq!(allowed_edits(6), 1);
        assert_eq!(allowed_edits(7), 2);
    }

    #[test]
    fn exact_prefix_typo() {
        assert_eq!(q("brizo").score_fuzzy("brizo"), EXACT);
        assert_eq!(q("br").score_fuzzy("brizo"), PREFIX);
        assert_eq!(q("belris").score_fuzzy("belaris"), TYPO);
        assert_eq!(q("amaor").score_fuzzy("amaro"), TYPO);
        // A typo in a prefix: "pjesk" against "pjeshke".
        assert_eq!(q("pjesk").score_fuzzy("pjeshke"), TYPO);
        assert_eq!(q("vreshtarai").score_fuzzy("vreshtaria"), TYPO);
        assert_eq!(q("bra").score_fuzzy("bro"), 0, "short tokens get no typos");
        assert_eq!(q("zzzzzz").score_fuzzy("brizo"), 0);
        assert_eq!(q("belris").score_exact("belaris"), 0);
        assert_eq!(q("l0009").score_exact("l0009"), EXACT);
    }

    #[test]
    fn non_ascii_tokens_do_not_panic() {
        assert_eq!(q("ђђђђ").score_fuzzy("ђђђђђ"), PREFIX);
        assert_eq!(q("ђђђж").score_fuzzy("ђђђђђ"), TYPO);
        assert_eq!(q("abcd").score_fuzzy("ђ"), 0);
    }

    /// The plain definition, without the bag bound or the single pass.
    fn typo_match_reference(q: &str, t: &str) -> bool {
        let n = q.chars().count();
        let k = allowed_edits(n);
        let t_chars: Vec<char> = t.chars().collect();
        let mut slices: Vec<String> = [n.saturating_sub(1), n, n + 1]
            .into_iter()
            .filter(|&l| l > 0 && l < t_chars.len())
            .map(|l| t_chars[..l].iter().collect())
            .collect();
        if t_chars.len().abs_diff(n) <= k {
            slices.push(t.to_owned());
        }
        k > 0
            && slices
                .iter()
                .any(|s| strsim::damerau_levenshtein(q, s) <= k)
    }

    #[test]
    fn typo_match_agrees_with_the_definition() {
        let words = [
            "amaro",
            "amaor",
            "belris",
            "belaris",
            "brizo",
            "bryzo",
            "pjeshke",
            "pjesk",
            "pjeshk",
            "vreshtaria",
            "vreshtarai",
            "vrshtaria",
            "abcd",
            "abdc",
            "bacd",
            "ab",
            "abcdefgh",
            "abcdefghij",
            "hgfedcba",
            "kamez",
            "kamza",
            "tirane",
            "tiranes",
            "ђђђђ",
            "ђђђж",
            "x",
        ];
        for q in words {
            for t in words {
                let token = QueryToken::new(q.to_owned());
                assert_eq!(
                    token.allowed > 0 && token.typo_match(t),
                    typo_match_reference(q, t),
                    "{q} vs {t}"
                );
            }
        }
    }
}
