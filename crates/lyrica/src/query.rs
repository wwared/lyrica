//! Search queries (`architecture.md` → "Search" → "Query language").

use std::{cmp::Reverse, fmt};

use lyrica_mpd::{Filter, Tag};
use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{is_not, tag, take_while1},
    character::complete::{alpha1, anychar, char, satisfy},
    combinator::{map, map_opt, opt, peek, recognize, value},
    error::{ErrorKind, ParseError},
    multi::fold_many0,
    sequence::{pair, preceded, terminated},
};

const DEFAULT_FIELDS: [Tag; 5] = [
    Tag::File,
    Tag::Title,
    Tag::Artist,
    Tag::AlbumArtist,
    Tag::Album,
];

const FIELDS: [(&str, Tag); 6] = [
    ("title", Tag::Title),
    ("artist", Tag::Artist),
    ("albumartist", Tag::AlbumArtist),
    ("album", Tag::Album),
    ("file", Tag::File),
    ("any", Tag::Any),
];

/// In front of every user regex (user decision): Unicode `\w`, `\b` and
/// friends; a match limit, since MPD can't cancel a running scan.
const USER_REGEX: &str = "(*UTF)(*UCP)(*LIMIT_MATCH=10000)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryError {
    Blank,
    MissingQuote,
    MissingSlash,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            QueryError::Blank => "Empty search",
            QueryError::MissingQuote => "Missing closing \"",
            QueryError::MissingSlash => "Missing closing /",
        })
    }
}

impl std::error::Error for QueryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Term {
    negated: bool,
    /// `None`: the default fields.
    field: Option<Tag>,
    value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Contains(String),
    StartsWith(String),
    Regex(String),
}

/// Filter for `query`: every term holds.
pub fn compile(query: &str) -> Result<Filter, QueryError> {
    let mut terms = parse(query)?;
    if terms.is_empty() {
        return Err(QueryError::Blank);
    }
    // MPD's AND stops at the first false operand: likely rarest first.
    terms.sort_by_key(|term| match (term.negated, &term.value) {
        (false, Value::Contains(text) | Value::StartsWith(text)) => {
            (0, Reverse(text.chars().count()))
        }
        (false, Value::Regex(_)) => (1, Reverse(0)),
        (true, _) => (2, Reverse(0)),
    });
    Ok(Filter::And(terms.iter().map(Term::filter).collect()))
}

impl Term {
    fn filter(&self) -> Filter {
        let pattern = match &self.value {
            Value::Contains(text) => literal("", text),
            Value::StartsWith(text) => literal("^", text),
            Value::Regex(regex) => format!("{USER_REGEX}{regex}"),
        };
        let Some(tag) = self.field else {
            // MPD 0.24 has no OR: "one field matches" is "not every field misses".
            let every_miss = Filter::And(
                DEFAULT_FIELDS
                    .map(|tag| Filter::Regex {
                        tag,
                        pattern: pattern.clone(),
                        negated: true,
                    })
                    .into(),
            );
            return if self.negated {
                every_miss
            } else {
                Filter::Not(Box::new(every_miss))
            };
        };
        Filter::Regex {
            tag,
            pattern,
            negated: self.negated,
        }
    }
}

/// Regex matching `text` after `anchor`. `(*UTF)` gives Unicode case folding;
/// a backslash before an ASCII alphanumeric would start an escape sequence.
fn literal(anchor: &str, text: &str) -> String {
    const UTF: &str = "(*UTF)";
    let mut pattern = String::with_capacity(UTF.len() + anchor.len() + 2 * text.len());
    pattern.push_str(UTF);
    pattern.push_str(anchor);
    for c in text.chars() {
        if c.is_ascii() && !c.is_ascii_alphanumeric() {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

/// `Unclosed` ends the parse; `Backtrack` only steers `alt` and `opt`.
#[derive(Debug)]
enum Error {
    Unclosed(QueryError),
    Backtrack,
}

impl ParseError<&str> for Error {
    fn from_error_kind(_: &str, _: ErrorKind) -> Self {
        Error::Backtrack
    }

    fn append(_: &str, _: ErrorKind, other: Self) -> Self {
        other
    }
}

type Res<'a, T> = IResult<&'a str, T, Error>;

/// Terms in query order, empty values dropped.
fn parse(query: &str) -> Result<Vec<Term>, QueryError> {
    let mut terms = Vec::new();
    let mut rest = spaces(query);
    while !rest.is_empty() {
        let (after, parsed) = term(rest).map_err(|error| match error {
            nom::Err::Failure(Error::Unclosed(error)) => error,
            _ => unreachable!("every non-space text starts a term"),
        })?;
        terms.extend(parsed);
        rest = spaces(after);
    }
    Ok(terms)
}

fn spaces(input: &str) -> &str {
    input.trim_start_matches(char::is_whitespace)
}

/// `None` for an empty value. Only fails on an unclosed `"` or `/`.
fn term(input: &str) -> Res<'_, Option<Term>> {
    let (input, negated) = opt(prefix(char('-'))).parse(input)?;
    let (input, field) = opt(prefix(terminated(
        map_opt(alpha1, |name: &str| {
            FIELDS
                .iter()
                .find(|(field, _)| field.eq_ignore_ascii_case(name))
                .map(|&(_, tag)| tag)
        }),
        char(':'),
    )))
    .parse(input)?;
    let (input, value) = alt((
        map(quoted, Value::Contains),
        map(regex, Value::Regex),
        map(
            preceded(prefix(char('^')), alt((quoted, word))),
            Value::StartsWith,
        ),
        map(word, Value::Contains),
    ))
    .parse(input)?;
    let empty = match &value {
        Value::Contains(text) | Value::StartsWith(text) | Value::Regex(text) => text.is_empty(),
    };
    let term = Term {
        negated: negated.is_some(),
        field,
        value,
    };
    Ok((input, (!empty).then_some(term)))
}

/// `parser` with a value after it; alone it is plain text.
fn prefix<'a, O>(
    parser: impl Parser<&'a str, Output = O, Error = Error>,
) -> impl Parser<&'a str, Output = O, Error = Error> {
    terminated(parser, peek(satisfy(|c| !c.is_whitespace())))
}

fn word(input: &str) -> Res<'_, String> {
    map(take_while1(|c: char| !c.is_whitespace()), str::to_owned).parse(input)
}

/// `"text"`: `\"` is `"`, `\\` is `\`, other backslashes literal.
fn quoted(input: &str) -> Res<'_, String> {
    delimited(
        '"',
        alt((
            is_not("\\\""),
            preceded(char('\\'), recognize(satisfy(|c| c == '"' || c == '\\'))),
            tag("\\"),
        )),
        QueryError::MissingQuote,
    )
    .parse(input)
}

/// `/regex/`: `\/` is `/`, other backslash pairs kept whole.
fn regex(input: &str) -> Res<'_, String> {
    delimited(
        '/',
        alt((
            is_not("\\/"),
            value("/", tag("\\/")),
            recognize(pair(char('\\'), anychar)),
        )),
        QueryError::MissingSlash,
    )
    .parse(input)
}

/// `delimiter` text `delimiter`, the text joined from `piece`s; once open,
/// a missing closing delimiter is `unclosed`.
fn delimited<'a>(
    delimiter: char,
    piece: impl Parser<&'a str, Output = &'a str, Error = Error>,
    unclosed: QueryError,
) -> impl Parser<&'a str, Output = String, Error = Error> {
    preceded(
        char(delimiter),
        terminated(
            fold_many0(piece, String::new, |mut text, piece| {
                text.push_str(piece);
                text
            }),
            move |input| {
                char::<_, Error>(delimiter)
                    .parse(input)
                    .map_err(|_| nom::Err::Failure(Error::Unclosed(unclosed)))
            },
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(query: &str) -> String {
        compile(query).unwrap().to_string()
    }

    /// Rendered default-fields term for a pattern as it appears inside the
    /// filter's quotes, `(*UTF)` included.
    fn default_text(negated: bool, quoted: &str) -> String {
        let fields = ["file", "title", "artist", "albumartist", "album"]
            .map(|tag| format!("({tag} !~ '{quoted}')"))
            .join(" AND ");
        if negated {
            format!("({fields})")
        } else {
            format!("(!({fields}))")
        }
    }

    /// Rendered positive default-fields word term.
    fn term_text(quoted: &str) -> String {
        default_text(false, &format!("(*UTF){quoted}"))
    }

    fn term(negated: bool, field: Option<Tag>, value: Value) -> Term {
        Term {
            negated,
            field,
            value,
        }
    }

    fn contains(text: &str) -> Term {
        term(false, None, Value::Contains(text.into()))
    }

    fn starts(text: &str) -> Term {
        term(false, None, Value::StartsWith(text.into()))
    }

    fn regex(text: &str) -> Term {
        term(false, None, Value::Regex(text.into()))
    }

    fn not(term: Term) -> Term {
        Term {
            negated: true,
            ..term
        }
    }

    fn on(tag: Tag, term: Term) -> Term {
        Term {
            field: Some(tag),
            ..term
        }
    }

    fn terms(query: &str) -> Vec<Term> {
        parse(query).unwrap()
    }

    #[test]
    fn words_split_on_unicode_whitespace() {
        assert_eq!(
            terms(" \t radiohead\n\r\u{3000}creep\u{a0} "),
            [contains("radiohead"), contains("creep")]
        );
    }

    #[test]
    fn quoted_text_keeps_spaces_and_unescapes_quote_and_backslash() {
        assert_eq!(terms(r#""no  surprises""#), [contains("no  surprises")]);
        assert_eq!(terms(r#""a\"b""#), [contains(r#"a"b"#)]);
        assert_eq!(terms(r#""a\\b""#), [contains(r"a\b")]);
        assert_eq!(terms(r#""a\\""#), [contains(r"a\")]);
        assert_eq!(terms(r#""\\\"""#), [contains(r#"\""#)]);
        // Other backslashes are literal.
        assert_eq!(terms(r#""a\nb\/""#), [contains(r"a\nb\/")]);
    }

    #[test]
    fn closing_delimiter_ends_the_term() {
        assert_eq!(terms(r#""a"b"#), [contains("a"), contains("b")]);
        assert_eq!(terms(r#""a""b""#), [contains("a"), contains("b")]);
        assert_eq!(terms(r#""a"-b"#), [contains("a"), not(contains("b"))]);
        assert_eq!(terms("/x/i"), [regex("x"), contains("i")]);
        assert_eq!(terms(r#"/x/"y""#), [regex("x"), contains("y")]);
    }

    #[test]
    fn regex_unescapes_slash_only() {
        assert_eq!(terms("/world wide/"), [regex("world wide")]);
        assert_eq!(terms(r"/a\/b/"), [regex("a/b")]);
        // `\\` is kept whole, so the last `/` closes.
        assert_eq!(terms(r"/a\\/"), [regex(r"a\\")]);
        assert_eq!(terms(r"/\d+\.flac$/"), [regex(r"\d+\.flac$")]);
        assert_eq!(terms(r#"/a"b/"#), [regex(r#"a"b"#)]);
    }

    #[test]
    fn caret_starts_with_a_word_or_quoted_text() {
        assert_eq!(terms("^hello"), [starts("hello")]);
        assert_eq!(terms(r#"^"no surprises""#), [starts("no surprises")]);
        assert_eq!(terms("^/x/"), [starts("/x/")]);
        assert_eq!(terms("^-x"), [starts("-x")]);
        assert_eq!(terms("^^x"), [starts("^x")]);
    }

    #[test]
    fn lone_caret_or_dash_is_plain_text() {
        assert_eq!(terms("^"), [contains("^")]);
        assert_eq!(terms("^ x"), [contains("^"), contains("x")]);
        assert_eq!(terms("-"), [contains("-")]);
        assert_eq!(terms("- x"), [contains("-"), contains("x")]);
        assert_eq!(terms("-^"), [not(contains("^"))]);
        assert_eq!(terms("artist:^"), [on(Tag::Artist, contains("^"))]);
    }

    #[test]
    fn dash_negates_any_term_only_in_front() {
        assert_eq!(terms("-live"), [not(contains("live"))]);
        assert_eq!(terms(r#"-"a b""#), [not(contains("a b"))]);
        assert_eq!(terms("-/x/"), [not(regex("x"))]);
        assert_eq!(terms("-^the"), [not(starts("the"))]);
        assert_eq!(terms("-artist:x"), [not(on(Tag::Artist, contains("x")))]);
        assert_eq!(terms("--x"), [not(contains("-x"))]);
        assert_eq!(terms("a-b"), [contains("a-b")]);
        assert_eq!(terms("artist:-live"), [on(Tag::Artist, contains("-live"))]);
    }

    #[test]
    fn field_names_in_any_case() {
        for (name, tag) in [
            ("title", Tag::Title),
            ("artist", Tag::Artist),
            ("albumartist", Tag::AlbumArtist),
            ("album", Tag::Album),
            ("file", Tag::File),
            ("any", Tag::Any),
        ] {
            for name in [name.to_owned(), name.to_uppercase()] {
                assert_eq!(terms(&format!("{name}:x")), [on(tag, contains("x"))]);
            }
        }
        assert_eq!(
            terms("AlbumArtist:x"),
            [on(Tag::AlbumArtist, contains("x"))]
        );
    }

    #[test]
    fn field_takes_every_value_kind() {
        assert_eq!(terms(r#"artist:"a b""#), [on(Tag::Artist, contains("a b"))]);
        assert_eq!(terms("title:^x"), [on(Tag::Title, starts("x"))]);
        assert_eq!(terms(r"file:/\.flac$/"), [on(Tag::File, regex(r"\.flac$"))]);
        // A value is never a field again.
        assert_eq!(
            terms("artist:album:x"),
            [on(Tag::Artist, contains("album:x"))]
        );
        assert_eq!(terms("artist::x"), [on(Tag::Artist, contains(":x"))]);
    }

    #[test]
    fn unknown_or_valueless_field_is_plain_text() {
        assert_eq!(terms("re:zero"), [contains("re:zero")]);
        assert_eq!(terms("artists:x"), [contains("artists:x")]);
        assert_eq!(terms("ärtist:x"), [contains("ärtist:x")]);
        assert_eq!(terms("foo:/x/"), [contains("foo:/x/")]);
        assert_eq!(terms("artist:"), [contains("artist:")]);
        assert_eq!(terms("artist: x"), [contains("artist:"), contains("x")]);
        assert_eq!(terms("-artist:"), [not(contains("artist:"))]);
    }

    #[test]
    fn quote_or_slash_inside_a_word_is_plain_text() {
        assert_eq!(terms("AC/DC"), [contains("AC/DC")]);
        assert_eq!(terms("a/b/"), [contains("a/b/")]);
        assert_eq!(terms(r#"foo"bar"#), [contains(r#"foo"bar"#)]);
        assert_eq!(terms(r#"a""#), [contains(r#"a""#)]);
    }

    #[test]
    fn empty_values_are_dropped() {
        for query in [
            r#""""#,
            r#"^"""#,
            "//",
            r#"-"""#,
            "-//",
            r#"artist:"""#,
            "file://",
        ] {
            assert_eq!(terms(query), [], "{query}");
        }
        assert_eq!(terms(r#""" x // -^"""#), [contains("x")]);
        assert_eq!(compile(r#""" -// ^"""#), Err(QueryError::Blank));
    }

    #[test]
    fn blank_is_an_error() {
        assert_eq!(compile(""), Err(QueryError::Blank));
        assert_eq!(compile(" \t\n\u{3000}"), Err(QueryError::Blank));
    }

    #[test]
    fn unclosed_quote_or_slash_is_an_error() {
        for query in [
            r#"""#,
            r#""abc"#,
            r#""abc\""#,
            r#""abc\\\""#,
            r#"x "y z"#,
            r#"artist:"x"#,
            r#"-""#,
            r#"^""#,
        ] {
            assert_eq!(compile(query), Err(QueryError::MissingQuote), "{query}");
        }
        for query in ["/", "/abc", r"/abc\/", r"/abc\", "-/", "title:/x", "x /y z"] {
            assert_eq!(compile(query), Err(QueryError::MissingSlash), "{query}");
        }
        assert_eq!(QueryError::MissingQuote.to_string(), "Missing closing \"");
        assert_eq!(QueryError::MissingSlash.to_string(), "Missing closing /");
    }

    #[test]
    fn every_query_parses_or_fails_cleanly() {
        const PIECES: [&str; 11] = [
            "-", "^", "\"", "/", "\\", ":", " ", "a", "artist:", "é", "\u{3000}",
        ];
        let mut queries = vec![String::new()];
        for _ in 0..5 {
            queries = queries
                .iter()
                .flat_map(|query| PIECES.map(|piece| format!("{query}{piece}")))
                .collect();
            for query in &queries {
                if let Ok(filter) = compile(query) {
                    assert!(filter.to_string().starts_with('('), "{query}");
                }
            }
        }
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
    fn value_kinds_render_their_patterns() {
        assert_eq!(text("^hello"), term_text("^hello"));
        assert_eq!(text("^a.b"), term_text(r"^a\\.b"));
        assert_eq!(text(r#""a b""#), term_text(r"a\\ b"));
        // User regexes pass through verbatim after the prefix.
        assert_eq!(
            text(r"/\bé.*(?-i)X\'/"),
            default_text(false, r"(*UTF)(*UCP)(*LIMIT_MATCH=10000)\\bé.*(?-i)X\\\'")
        );
        assert_eq!(text("-love"), default_text(true, "(*UTF)love"));
        assert_eq!(text("any:love"), "(any =~ '(*UTF)love')");
        assert_eq!(text("-title:^love"), "(title !~ '(*UTF)^love')");
    }

    #[test]
    fn literals_longest_first_then_regexes_then_negated() {
        assert_eq!(
            text(r#"-zz /b/ ab artist:creep /a/ ^radiohead -y "x y z w""#),
            format!(
                "({} AND {} AND {} AND {} AND {} AND {} AND {} AND {})",
                term_text("^radiohead"),
                term_text(r"x\\ y\\ z\\ w"),
                "(artist =~ '(*UTF)creep')",
                term_text("ab"),
                default_text(false, "(*UTF)(*UCP)(*LIMIT_MATCH=10000)b"),
                default_text(false, "(*UTF)(*UCP)(*LIMIT_MATCH=10000)a"),
                default_text(true, "(*UTF)zz"),
                default_text(true, "(*UTF)y"),
            )
        );
        // Ties in query order.
        assert_eq!(
            text("ab cd"),
            format!("({} AND {})", term_text("ab"), term_text("cd"))
        );
        // Characters, not bytes: "ööö" is 6 bytes.
        assert_eq!(
            text("abcd ööö"),
            format!("({} AND {})", term_text("abcd"), term_text("ööö"))
        );
    }

    #[test]
    fn literals_escape_ascii_punctuation() {
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
    fn literals_keep_letters_digits_and_non_ascii() {
        assert_eq!(text("BJÖRK"), term_text("BJÖRK"));
        assert_eq!(text("Σ"), term_text("Σ"));
        assert_eq!(text("マクロス82"), term_text("マクロス82"));
        assert_eq!(text("a—b"), term_text("a—b"));
    }

    /// `architecture.md` → "Query language", the example table.
    #[test]
    fn docs_examples() {
        let user = "(*UTF)(*UCP)(*LIMIT_MATCH=10000)";
        for (query, filter) in [
            (
                "radiohead creep",
                format!("({} AND {})", term_text("radiohead"), term_text("creep")),
            ),
            (r#""no surprises""#, term_text(r"no\\ surprises")),
            (
                "-live ^hello /world wide/",
                format!(
                    "({} AND {} AND {})",
                    term_text("^hello"),
                    default_text(false, &format!("{user}world wide")),
                    default_text(true, "(*UTF)live"),
                ),
            ),
            (
                "artist:radiohead -album:live",
                "((artist =~ '(*UTF)radiohead') AND (album !~ '(*UTF)live'))".into(),
            ),
            ("-^the", default_text(true, "(*UTF)^the")),
            (r"file:/\.flac$/", format!(r"(file =~ '{user}\\.flac$')")),
            (
                "/(?-i)LOVE/",
                default_text(false, &format!("{user}(?-i)LOVE")),
            ),
            ("re:zero", term_text(r"re\\:zero")),
        ] {
            assert_eq!(text(query), filter, "{query}");
        }
    }
}
