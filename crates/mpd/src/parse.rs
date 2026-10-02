use std::{str::FromStr, time::Duration};

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{tag, take_until},
    character::complete::{char, u32},
    combinator::{all_consuming, map, rest, value, verify},
    sequence::{preceded, separated_pair},
};

use crate::{Ack, Error, Result, Version};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Line<'a> {
    Ok,
    Ack(Ack),
    Pair(&'a str, &'a str),
}

pub(crate) fn greeting(input: &str) -> Option<Version> {
    let result: IResult<&str, _> = all_consuming(preceded(
        tag("OK MPD "),
        (u32, char('.'), u32, char('.'), u32),
    ))
    .parse(input);
    let (_, (major, _, minor, _, patch)) = result.ok()?;
    Some(Version {
        major,
        minor,
        patch,
    })
}

pub(crate) fn line(input: &str) -> Option<Line<'_>> {
    alt((
        value(Line::Ok, all_consuming(tag("OK"))),
        map(ack, Line::Ack),
        map(pair_parser, |(key, value)| Line::Pair(key, value)),
    ))
    .parse(input)
    .ok()
    .map(|(_, line)| line)
}

pub(crate) fn pair(input: &str) -> Option<(&str, &str)> {
    pair_parser(input).ok().map(|(_, pair)| pair)
}

fn pair_parser(input: &str) -> IResult<&str, (&str, &str)> {
    separated_pair(
        verify(take_until(": "), |key: &str| !key.is_empty()),
        tag(": "),
        rest,
    )
    .parse(input)
}

fn ack(input: &str) -> IResult<&str, Ack> {
    map(
        (
            tag("ACK ["),
            u32,
            char('@'),
            u32,
            tag("] {"),
            take_until("}"),
            tag("} "),
            rest,
        ),
        |(_, code, _, index, _, command, _, message): (_, _, _, _, _, &str, _, &str)| Ack {
            code,
            index,
            command: command.to_owned(),
            message: message.to_owned(),
        },
    )
    .parse(input)
}

pub(crate) fn field<T: FromStr>(key: &str, value: &str) -> Result<T> {
    value.parse().map_err(|_| invalid(key, value))
}

pub(crate) fn seconds(key: &str, value: &str) -> Result<Duration> {
    Duration::try_from_secs_f64(field(key, value)?).map_err(|_| invalid(key, value))
}

pub(crate) fn invalid(key: &str, value: &str) -> Error {
    Error::Protocol(format!("invalid {key}: {value:?}"))
}

pub(crate) fn missing(key: &str) -> Error {
    Error::Protocol(format!("missing {key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_version() {
        assert_eq!(
            greeting("OK MPD 0.24.0"),
            Some(Version {
                major: 0,
                minor: 24,
                patch: 0
            })
        );
        assert_eq!(greeting("OK MPD 0.24"), None);
        assert_eq!(greeting("OK MPD 0.24.0 extra"), None);
        assert_eq!(greeting("ACK [5@0] {} nope"), None);
    }

    #[test]
    fn response_lines() {
        assert_eq!(line("OK"), Some(Line::Ok));
        assert_eq!(
            line(r#"ACK [5@0] {} unknown command "foo""#),
            Some(Line::Ack(Ack {
                code: 5,
                index: 0,
                command: String::new(),
                message: r#"unknown command "foo""#.into(),
            }))
        );
        assert_eq!(
            line("ACK [2@3] {play} Bad song index"),
            Some(Line::Ack(Ack {
                code: 2,
                index: 3,
                command: "play".into(),
                message: "Bad song index".into(),
            }))
        );
        assert_eq!(
            line("file: a: b/c.flac"),
            Some(Line::Pair("file", "a: b/c.flac"))
        );
        assert_eq!(line("Title: "), Some(Line::Pair("Title", "")));
        assert_eq!(line("OK MPD 0.24.0"), None);
        assert_eq!(line(": value"), None);
        assert_eq!(line("garbage"), None);
    }

    #[test]
    fn seconds_rejects_negative() {
        assert_eq!(
            seconds("elapsed", "1.5").unwrap(),
            Duration::from_millis(1500)
        );
        assert!(seconds("elapsed", "-1").is_err());
        assert!(seconds("elapsed", "x").is_err());
    }
}
