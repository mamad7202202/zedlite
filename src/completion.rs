//! Lightweight, instant word-completion for the editor.
//!
//! Two suggestion sources feed the inline ghost text:
//! 1. local: most frequent other words in the buffer that start with the
//!    current prefix (zero latency, works offline)
//! 2. AI: an on-demand model completion (Alt+\) routed through the hub

/// The identifier directly before the caret and where it starts (char cols).
pub fn word_prefix_at(line: &str, col: usize) -> (String, usize) {
    let chars: Vec<char> = line.chars().collect();
    let col = col.min(chars.len());
    let mut start = col;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let prefix: String = chars[start..col].iter().collect();
    (prefix, start)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Candidate words from a window of buffer lines that extend `prefix`.
/// Frequency-ranked, excludes the prefix itself.
pub fn collect_candidates(lines: &[String], cursor_row: usize, prefix: &str) -> Vec<String> {
    let pl = prefix.to_lowercase();
    if pl.chars().count() < 3 {
        return Vec::new();
    }
    // scan a window around the caret so huge files stay snappy
    let half = 200usize;
    let start_row = cursor_row.saturating_sub(half);
    let end_row = (cursor_row + half).min(lines.len());

    let mut freqs: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for line in &lines[start_row..end_row] {
        for token in split_words(line) {
            if token.len() > pl.len()
                && token.to_lowercase().starts_with(&pl)
                && !token.eq_ignore_ascii_case(prefix)
            {
                *freqs.entry(token).or_insert(0) += 1;
            }
        }
    }
    let mut scored: Vec<(String, usize)> = freqs.into_iter().collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.len().cmp(&b.0.len())));
    scored.into_iter().map(|(w, _)| w).take(6).collect()
}

fn split_words(line: &str) -> impl Iterator<Item = String> + '_ {
    line.split(|c: char| !is_word(c))
        .filter(|w| w.len() >= 4 && w.len() < 40)
        .map(|w| w.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_extraction() {
        let (p, s) = word_prefix_at("hello wor", 9);
        assert_eq!(p, "wor");
        assert_eq!(s, 6);
        let (p2, _) = word_prefix_at("hello ", 6);
        assert_eq!(p2, "");
    }

    #[test]
    fn candidates_rank_and_exclude_prefix() {
        let lines = vec![
            "workspace_width".to_string(),
            "workspace_height".to_string(),
            "workspace".to_string(),
            "unrelated".to_string(),
        ];
        let got = collect_candidates(&lines, 0, "work");
        assert_eq!(got.first().map(|s| s.as_str()), Some("workspace"));
        assert!(!got.iter().any(|w| w == "work"));
    }
}
