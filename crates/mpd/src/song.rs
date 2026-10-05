use std::{mem, time::Duration};

use crate::{
    Result,
    parse::{field, missing, seconds},
};

/// A song as returned by `currentsong` or `playlistinfo`.
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
    /// `None` also when the tag isn't a plain number.
    pub track: Option<u32>,
    pub duration: Option<Duration>,
    pub id: Option<u32>,
}

impl Song {
    /// `None` for an empty response (no current song).
    pub(crate) fn from_pairs<'a>(
        pairs: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> Result<Option<Self>> {
        let mut fields = Fields::default();
        let mut empty = true;
        for (key, value) in pairs {
            empty = false;
            fields.set(key, value)?;
        }
        if empty {
            return Ok(None);
        }
        fields.finish().map(Some)
    }

    /// Songs of a multi-song response; each starts at its `file` line.
    pub(crate) fn list_from_pairs<'a>(
        pairs: impl Iterator<Item = (&'a str, &'a str)>,
        capacity: usize,
    ) -> Result<Vec<Self>> {
        let mut songs = Vec::with_capacity(capacity);
        let mut fields = Fields::default();
        let mut empty = true;
        for (key, value) in pairs {
            if key == "file" && fields.file.is_some() {
                songs.push(mem::take(&mut fields).finish()?);
            }
            empty = false;
            fields.set(key, value)?;
        }
        if !empty {
            songs.push(fields.finish()?);
        }
        Ok(songs)
    }
}

#[derive(Default)]
struct Fields {
    file: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album_artist: Option<String>,
    album: Option<String>,
    track: Option<u32>,
    track_seen: bool,
    duration: Option<Duration>,
    id: Option<u32>,
}

impl Fields {
    fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "file" => self.file = Some(value.to_owned()),
            "Title" => first(&mut self.title, value),
            "Artist" => first(&mut self.artist, value),
            "AlbumArtist" => first(&mut self.album_artist, value),
            "Album" => first(&mut self.album, value),
            // Free-form tag; one odd value mustn't fail a whole queue window.
            "Track" if !self.track_seen => {
                self.track_seen = true;
                self.track = value.parse().ok();
            }
            "duration" => self.duration = Some(seconds(key, value)?),
            "Id" => self.id = Some(field(key, value)?),
            _ => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<Song> {
        Ok(Song {
            file: self.file.ok_or_else(|| missing("file"))?,
            title: self.title,
            artist: self.artist,
            album_artist: self.album_artist,
            album: self.album,
            track: self.track,
            duration: self.duration,
            id: self.id,
        })
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

    fn parse_list(text: &str) -> Result<Vec<Song>> {
        Song::list_from_pairs(text.lines().filter_map(pair), 0)
    }

    #[test]
    fn current_song() {
        let song = parse(
            "file: Artist A/Album A/01 Song A.flac\nLast-Modified: 2020-01-01T00:00:00Z\n\
             Format: 44100:f:2\nArtist: Artist A\nArtist: Other\nAlbum: Album A\n\
             Title: Song A\nTrack: 3\nTrack: 4\nDate: 2000\nAlbumArtist: Artist A\nTime: 200\n\
             duration: 200.250\nPos: 41\nId: 42",
        )
        .unwrap();
        assert_eq!(
            song,
            Some(Song {
                file: "Artist A/Album A/01 Song A.flac".into(),
                title: Some("Song A".into()),
                artist: Some("Artist A".into()),
                album_artist: Some("Artist A".into()),
                album: Some("Album A".into()),
                track: Some(3),
                duration: Some(Duration::from_millis(200_250)),
                id: Some(42),
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
        assert!(matches!(parse_list("Title: x"), Err(Error::Protocol(_))));
    }

    #[test]
    fn song_list() {
        let songs = parse_list(
            "file: a/1.flac\nTitle: One\nTrack: A1\nTrack: 1\nPos: 7\nId: 8\n\
             file: b/2.flac\nPos: 8\nId: 9\n\
             file: c/3.flac\nArtist: C\nTrack: 3\nduration: 1.5\nPos: 9\nId: 10",
        )
        .unwrap();
        let files: Vec<_> = songs.iter().map(|s| s.file.as_str()).collect();
        assert_eq!(files, ["a/1.flac", "b/2.flac", "c/3.flac"]);
        assert_eq!(songs[0].title.as_deref(), Some("One"));
        assert_eq!(songs[0].track, None);
        assert_eq!(
            songs[1],
            Song {
                file: "b/2.flac".into(),
                title: None,
                artist: None,
                album_artist: None,
                album: None,
                track: None,
                duration: None,
                id: Some(9),
            }
        );
        assert_eq!(songs[2].artist.as_deref(), Some("C"));
        assert_eq!(songs[2].track, Some(3));
        assert_eq!(songs[2].duration, Some(Duration::from_millis(1500)));
        assert_eq!(songs[2].id, Some(10));
        assert!(parse_list("").unwrap().is_empty());
    }

    #[test]
    fn song_list_rejects_bad_fields() {
        assert!(matches!(
            parse_list("file: a\nId: 1\nfile: b\nId: x"),
            Err(Error::Protocol(_))
        ));
    }
}
