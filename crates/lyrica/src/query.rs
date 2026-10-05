//! Search queries (`architecture.md` → "Search" → "Query language"), phase 1:
//! plain words.

use std::{cmp::Reverse, fmt};

use lyrica_mpd::{Filter, Tag};

const DEFAULT_FIELDS: [Tag; 5] = [
    Tag::File,
    Tag::Title,
    Tag::Artist,
    Tag::AlbumArtist,
    Tag::Album,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryError {
    Blank,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::Blank => f.write_str("Empty search"),
        }
    }
}

impl std::error::Error for QueryError {}

/// Filter for `query`: every word is a substring of a default field.
pub fn compile(query: &str) -> Result<Filter, QueryError> {
    let mut words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Err(QueryError::Blank);
    }
    // MPD's AND stops at the first false operand; longer is likely rarer.
    words.sort_by_key(|word| Reverse(word.chars().count()));
    Ok(Filter::And(words.into_iter().map(term).collect()))
}

/// Matches when one default field contains `word`; MPD 0.24 has no OR.
fn term(word: &str) -> Filter {
    let pattern = literal(word);
    Filter::Not(Box::new(Filter::And(
        DEFAULT_FIELDS
            .map(|tag| Filter::Regex {
                tag,
                pattern: pattern.clone(),
                negated: true,
            })
            .into(),
    )))
}

/// Regex matching `word` anywhere. `(*UTF)` gives Unicode case folding; a
/// backslash before an ASCII alphanumeric would start an escape sequence.
fn literal(word: &str) -> String {
    const UTF: &str = "(*UTF)";
    let mut pattern = String::with_capacity(UTF.len() + 2 * word.len());
    pattern.push_str(UTF);
    for c in word.chars() {
        if c.is_ascii() && !c.is_ascii_alphanumeric() {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(query: &str) -> String {
        compile(query).unwrap().to_string()
    }

    /// Rendered term for a pattern as it appears inside the filter's quotes.
    fn term_text(quoted: &str) -> String {
        let fields = ["file", "title", "artist", "albumartist", "album"]
            .map(|tag| format!("({tag} !~ '(*UTF){quoted}')"))
            .join(" AND ");
        format!("(!({fields}))")
    }

    #[test]
    fn one_word_matches_any_default_field_file_first() {
        assert_eq!(
            text("love"),
            "(!((file !~ '(*UTF)love') AND (title !~ '(*UTF)love') AND \
             (artist !~ '(*UTF)love') AND (albumartist !~ '(*UTF)love') AND \
             (album !~ '(*UTF)love')))"
        );
    }

    #[test]
    fn words_longest_first_ties_in_query_order() {
        assert_eq!(
            text("ab creep cd radiohead"),
            format!(
                "({} AND {} AND {} AND {})",
                term_text("radiohead"),
                term_text("creep"),
                term_text("ab"),
                term_text("cd"),
            )
        );
        // Characters, not bytes: "ööö" is 6 bytes.
        assert_eq!(
            text("abcd ööö"),
            format!("({} AND {})", term_text("abcd"), term_text("ööö"))
        );
    }

    #[test]
    fn splits_on_unicode_whitespace() {
        assert_eq!(
            text(" \t radiohead\n\r\u{3000}creep\u{a0} "),
            text("radiohead creep")
        );
    }

    #[test]
    fn escapes_ascii_punctuation() {
        // Regex `it\'s`; the filter's quoting doubles the backslash and adds one for `'`.
        assert_eq!(text("it's"), term_text(r"it\\\'s"));
        assert_eq!(text("ac/dc"), term_text(r"ac\\/dc"));
        assert_eq!(text(r"a\b"), term_text(r"a\\\\b"));
        assert_eq!(
            text("a.b*c?(d)[e]{f}|^$+"),
            term_text(r"a\\.b\\*c\\?\\(d\\)\\[e\\]\\{f\\}\\|\\^\\$\\+")
        );
        assert_eq!(text("_-\"#"), term_text(r#"\\_\\-\\"\\#"#));
    }

    #[test]
    fn keeps_letters_digits_and_non_ascii() {
        assert_eq!(text("BJÖRK"), term_text("BJÖRK"));
        assert_eq!(text("Σ"), term_text("Σ"));
        assert_eq!(text("マクロス82"), term_text("マクロス82"));
        assert_eq!(text("a—b"), term_text("a—b"));
    }

    #[test]
    fn blank_is_an_error() {
        assert_eq!(compile(""), Err(QueryError::Blank));
        assert_eq!(compile(" \t\n\u{3000}"), Err(QueryError::Blank));
    }
}
