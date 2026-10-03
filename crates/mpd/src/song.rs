use std::time::Duration;

use crate::{
    Result,
    parse::{field, missing, seconds},
};

/// A song as returned by `currentsong`.
///
/// Multi-value tags (e.g. several `Artist` lines) keep only the first value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Song {
    /// URI relative to the music directory.
    pub file: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<Duration>,
    /// Queue position.
    pub position: Option<u32>,
    pub id: Option<u32>,
}

impl Song {
    /// `None` for an empty response (no current song).
    pub(crate) fn from_pairs<'a>(
        pairs: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> Result<Option<Self>> {
        let mut file = None;
        let mut title = None;
        let mut artist = None;
        let mut album_artist = None;
        let mut album = None;
        let mut duration = None;
        let mut position = None;
        let mut id = None;
        let mut empty = true;
        for (key, value) in pairs {
            empty = false;
            match key {
                "file" => file = Some(value.to_owned()),
                "Title" => first(&mut title, value),
                "Artist" => first(&mut artist, value),
                "AlbumArtist" => first(&mut album_artist, value),
                "Album" => first(&mut album, value),
                "duration" => duration = Some(seconds(key, value)?),
                "Pos" => position = Some(field(key, value)?),
                "Id" => id = Some(field(key, value)?),
                _ => {}
            }
        }
        if empty {
            return Ok(None);
        }
        Ok(Some(Song {
            file: file.ok_or_else(|| missing("file"))?,
            title,
            artist,
            album_artist,
            album,
            duration,
            position,
            id,
        }))
    }
}

fn first(slot: &mut Option<String>, value: &str) {
    slot.get_or_insert_with(|| value.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, parse::pair};

    fn parse(text: &str) -> Result<Option<Song>> {
        Song::from_pairs(text.lines().filter_map(pair))
    }

    #[test]
    fn current_song() {
        let song = parse(
            "file: FELT/00 Puppet in the Dark.m4a\nLast-Modified: 2025-05-14T21:34:59Z\n\
             Format: 44100:f:2\nArtist: FELT\nArtist: Other\nAlbum: FELT Special Page Contents\n\
             Title: Puppet in the Dark\nDate: 2021\nAlbumArtist: FELT\nTime: 306\n\
             duration: 305.784\nPos: 40609\nId: 40610",
        )
        .unwrap();
        assert_eq!(
            song,
            Some(Song {
                file: "FELT/00 Puppet in the Dark.m4a".into(),
                title: Some("Puppet in the Dark".into()),
                artist: Some("FELT".into()),
                album_artist: Some("FELT".into()),
                album: Some("FELT Special Page Contents".into()),
                duration: Some(Duration::from_millis(305_784)),
                position: Some(40609),
                id: Some(40610),
            })
        );
    }

    #[test]
    fn no_current_song() {
        assert_eq!(parse("").unwrap(), None);
    }

    #[test]
    fn missing_file() {
        assert!(matches!(parse("Title: x"), Err(Error::Protocol(_))));
    }
}
