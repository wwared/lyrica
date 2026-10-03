use std::time::Duration;

use crate::{
    Result,
    parse::{field, invalid, missing, seconds},
};

/// Response of `status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    /// Queue position of the current song.
    pub song: Option<u32>,
    pub song_id: Option<u32>,
    pub elapsed: Option<Duration>,
    pub duration: Option<Duration>,
    pub random: bool,
    pub repeat: bool,
    pub single: Single,
    /// Queue version; changes whenever the queue does.
    pub queue_version: u32,
    pub queue_length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Play,
    Pause,
    Stop,
}

/// `single` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Single {
    #[default]
    Off,
    On,
    /// On until the current song ends, then off.
    Oneshot,
}

impl Single {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Single::Off => "0",
            Single::On => "1",
            Single::Oneshot => "oneshot",
        }
    }
}

impl Status {
    pub(crate) fn from_pairs<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> Result<Self> {
        let mut state = None;
        let mut queue_version = None;
        let mut queue_length = None;
        let mut song = None;
        let mut song_id = None;
        let mut elapsed = None;
        let mut duration = None;
        let mut random = false;
        let mut repeat = false;
        let mut single = Single::Off;
        for (key, value) in pairs {
            match key {
                "state" => {
                    state = Some(match value {
                        "play" => State::Play,
                        "pause" => State::Pause,
                        "stop" => State::Stop,
                        _ => return Err(invalid(key, value)),
                    })
                }
                "playlist" => queue_version = Some(field(key, value)?),
                "playlistlength" => queue_length = Some(field(key, value)?),
                "song" => song = Some(field(key, value)?),
                "songid" => song_id = Some(field(key, value)?),
                "elapsed" => elapsed = Some(seconds(key, value)?),
                "duration" => duration = Some(seconds(key, value)?),
                "random" => random = flag(key, value)?,
                "repeat" => repeat = flag(key, value)?,
                "single" => {
                    single = match value {
                        "0" => Single::Off,
                        "1" => Single::On,
                        "oneshot" => Single::Oneshot,
                        _ => return Err(invalid(key, value)),
                    }
                }
                _ => {}
            }
        }
        Ok(Status {
            state: state.ok_or_else(|| missing("state"))?,
            song,
            song_id,
            elapsed,
            duration,
            random,
            repeat,
            single,
            queue_version: queue_version.ok_or_else(|| missing("playlist"))?,
            queue_length: queue_length.ok_or_else(|| missing("playlistlength"))?,
        })
    }
}

fn flag(key: &str, value: &str) -> Result<bool> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(invalid(key, value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, parse::pair};

    fn parse(text: &str) -> Result<Status> {
        Status::from_pairs(text.lines().filter_map(pair))
    }

    #[test]
    fn stopped() {
        let status = parse(
            "volume: 100\nrepeat: 1\nrandom: 1\nsingle: 0\nconsume: 0\npartition: default\n\
             playlist: 2\nplaylistlength: 175872\nmixrampdb: 0\nstate: stop\n\
             lastloadedplaylist: nonash\nsong: 40609\nsongid: 40610\nnextsong: 29770\nnextsongid: 29771",
        )
        .unwrap();
        assert_eq!(
            status,
            Status {
                state: State::Stop,
                song: Some(40609),
                song_id: Some(40610),
                elapsed: None,
                duration: None,
                random: true,
                repeat: true,
                single: Single::Off,
                queue_version: 2,
                queue_length: 175872,
            }
        );
    }

    #[test]
    fn playing() {
        let status = parse(
            "playlist: 7\nplaylistlength: 3\nstate: play\nsong: 1\nsongid: 2\n\
             time: 12:306\nelapsed: 12.345\nbitrate: 256\nduration: 305.784\naudio: 44100:f:2",
        )
        .unwrap();
        assert_eq!(status.state, State::Play);
        assert_eq!(status.elapsed, Some(Duration::from_millis(12_345)));
        assert_eq!(status.duration, Some(Duration::from_millis(305_784)));
        assert_eq!(
            parse("playlist: 7\nplaylistlength: 3\nstate: play\nsingle: oneshot\nrepeat: 0")
                .unwrap()
                .single,
            Single::Oneshot
        );
    }

    #[test]
    fn empty_queue() {
        let status = parse("playlist: 1\nplaylistlength: 0\nstate: stop").unwrap();
        assert_eq!(status.song, None);
        assert_eq!(status.queue_length, 0);
    }

    #[test]
    fn rejects_bad_fields() {
        assert!(matches!(
            parse("playlist: 1\nplaylistlength: 0"),
            Err(Error::Protocol(_))
        ));
        assert!(matches!(
            parse("playlist: 1\nplaylistlength: 0\nstate: playing"),
            Err(Error::Protocol(_))
        ));
        assert!(matches!(
            parse("playlist: 1\nplaylistlength: -1\nstate: stop"),
            Err(Error::Protocol(_))
        ));
        assert!(matches!(
            parse("playlist: 1\nplaylistlength: 0\nstate: stop\nrandom: yes"),
            Err(Error::Protocol(_))
        ));
    }
}
