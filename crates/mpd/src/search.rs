use std::fmt::{self, Write};

use crate::{
    Result,
    parse::{field, missing},
};

/// MPD filter expression (protocol "Filters"). `Display` renders it, with
/// regex patterns verbatim and quoted for the filter; the command argument's
/// own quoting is added when sending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// `(tag =~ 'pattern')`, `!~` when `negated`.
    Regex {
        tag: Tag,
        pattern: String,
        negated: bool,
    },
    /// `(!filter)`
    Not(Box<Filter>),
    /// `(a AND b …)`; one operand renders as itself. Rendering an empty one
    /// fails: MPD has no filter for it.
    And(Vec<Filter>),
}

/// Tags lyrica searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    File,
    Title,
    Artist,
    AlbumArtist,
    Album,
    /// Every tag and the file name.
    Any,
}

impl Tag {
    pub fn as_str(self) -> &'static str {
        match self {
            Tag::File => "file",
            Tag::Title => "title",
            Tag::Artist => "artist",
            Tag::AlbumArtist => "albumartist",
            Tag::Album => "album",
            Tag::Any => "any",
        }
    }
}

impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Filter::Regex {
                tag,
                pattern,
                negated,
            } => {
                let op = if *negated { "!~" } else { "=~" };
                write!(f, "({} {op} ", tag.as_str())?;
                quote(f, pattern)?;
                f.write_char(')')
            }
            Filter::Not(filter) => write!(f, "(!{filter})"),
            Filter::And(filters) => match filters.as_slice() {
                [] => Err(fmt::Error),
                [filter] => filter.fmt(f),
                [first, rest @ ..] => {
                    write!(f, "({first}")?;
                    for filter in rest {
                        write!(f, " AND {filter}")?;
                    }
                    f.write_char(')')
                }
            },
        }
    }
}

/// `value` in single quotes, `\` and `'` backslashed.
fn quote(f: &mut fmt::Formatter<'_>, value: &str) -> fmt::Result {
    f.write_char('\'')?;
    let mut rest = value;
    while let Some(i) = rest.find(['\\', '\'']) {
        f.write_str(&rest[..i])?;
        f.write_char('\\')?;
        f.write_str(&rest[i..=i])?;
        rest = &rest[i + 1..];
    }
    f.write_str(rest)?;
    f.write_char('\'')
}

/// Matches of a queue search, with the queue they were found in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueMatches {
    pub queue_version: u32,
    pub queue_length: u32,
    /// Queue positions, in queue order.
    pub positions: Vec<u32>,
}

impl QueueMatches {
    /// Pairs of `status` followed by `playlistsearch`. Split at the first
    /// `file`: both carry `duration`, and songs any tag.
    pub(crate) fn from_pairs<'a>(
        pairs: impl Iterator<Item = (&'a str, &'a str)>,
        capacity: usize,
    ) -> Result<Self> {
        let mut queue_version = None;
        let mut queue_length = None;
        let mut positions = Vec::with_capacity(capacity);
        let mut songs = false;
        let mut needs_pos = false;
        for (key, value) in pairs {
            match key {
                "file" => {
                    if needs_pos {
                        return Err(missing("Pos"));
                    }
                    songs = true;
                    needs_pos = true;
                }
                "Pos" if needs_pos => {
                    positions.push(field(key, value)?);
                    needs_pos = false;
                }
                "playlist" if !songs => queue_version = Some(field(key, value)?),
                "playlistlength" if !songs => queue_length = Some(field(key, value)?),
                _ => {}
            }
        }
        if needs_pos {
            return Err(missing("Pos"));
        }
        Ok(QueueMatches {
            queue_version: queue_version.ok_or_else(|| missing("playlist"))?,
            queue_length: queue_length.ok_or_else(|| missing("playlistlength"))?,
            positions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, parse::pair};

    fn regex(tag: Tag, pattern: &str, negated: bool) -> Filter {
        Filter::Regex {
            tag,
            pattern: pattern.into(),
            negated,
        }
    }

    #[test]
    fn renders_filters() {
        assert_eq!(
            regex(Tag::AlbumArtist, "(*UTF)björk", false).to_string(),
            "(albumartist =~ '(*UTF)björk')"
        );
        assert_eq!(
            Filter::Not(Box::new(regex(Tag::Any, "x", true))).to_string(),
            "(!(any !~ 'x'))"
        );
        assert_eq!(
            Filter::And(vec![regex(Tag::Title, "a", false)]).to_string(),
            "(title =~ 'a')"
        );
        let any_field = Filter::Not(Box::new(Filter::And(vec![
            regex(Tag::File, "r", true),
            regex(Tag::Title, "r", true),
            regex(Tag::Album, "r", true),
        ])));
        assert_eq!(
            Filter::And(vec![any_field, regex(Tag::Artist, "s", true)]).to_string(),
            "((!((file !~ 'r') AND (title !~ 'r') AND (album !~ 'r'))) AND (artist !~ 's'))"
        );
    }

    #[test]
    fn escapes_quote_and_backslash_only() {
        assert_eq!(
            regex(Tag::Title, r#"it\'s "a" \\ '"#, false).to_string(),
            r#"(title =~ 'it\\\'s "a" \\\\ \'')"#
        );
    }

    #[test]
    fn empty_and_fails() {
        let mut out = String::new();
        let filter = Filter::Not(Box::new(Filter::And(vec![])));
        assert!(write!(out, "{filter}").is_err());
    }

    fn parse(text: &str) -> Result<QueueMatches> {
        QueueMatches::from_pairs(text.lines().filter_map(pair), 0)
    }

    #[test]
    fn keeps_status_before_songs_and_positions() {
        let matches = parse(
            "playlist: 2\nplaylistlength: 175872\nduration: 247.106\n\
             file: a.mp3\nduration: 102.008\nplaylist: 9\nPos: 116\nId: 117\n\
             file: b.mp3\nPos: 180\nplaylistlength: 1",
        )
        .unwrap();
        assert_eq!(
            matches,
            QueueMatches {
                queue_version: 2,
                queue_length: 175872,
                positions: vec![116, 180],
            }
        );
        assert!(
            parse("playlist: 3\nplaylistlength: 0")
                .unwrap()
                .positions
                .is_empty()
        );
    }

    #[test]
    fn rejects_missing_fields() {
        for text in [
            "playlistlength: 1\nfile: a\nPos: 0",
            "playlist: 1\nfile: a\nplaylistlength: 1\nPos: 0",
            "playlist: 1\nplaylistlength: 2\nfile: a\nfile: b\nPos: 1",
            "playlist: 1\nplaylistlength: 2\nfile: a\nPos: 0\nfile: b",
            "playlist: 1\nplaylistlength: 2\nfile: a\nPos: x",
        ] {
            assert!(matches!(parse(text), Err(Error::Protocol(_))), "{text}");
        }
    }
}
