//! Queue search (`docs/architecture.md` → "Search" → "Server"): a query's
//! matches in chunks, and the first one after the current song.

use std::{sync::Arc, time::Instant};

use lyrica_mpd::{Client, Error, Filter, QueueMatches};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::bridge::{Mpd, execute};

/// Matches per chunk: `window k:k+500`, k a multiple of 500.
pub const CHUNK: u32 = 500;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Message<'a> {
    Matches {
        id: u32,
        version: Option<u32>,
        start: u32,
        positions: Option<&'a [u32]>,
        first: Option<u32>,
    },
    SearchFailed {
        id: u32,
        error: &'a str,
    },
    SearchStale {
        id: u32,
    },
}

impl Message<'_> {
    fn text(&self) -> String {
        serde_json::to_string(self).expect("search message serializes")
    }
}

fn matches(
    id: u32,
    version: Option<u32>,
    start: u32,
    positions: Option<&[u32]>,
    first: Option<u32>,
) -> String {
    Message::Matches {
        id,
        version,
        start,
        positions,
        first,
    }
    .text()
}

/// JSON `searchfailed` reply.
pub fn failed(id: u32, error: &str) -> String {
    Message::SearchFailed { id, error }.text()
}

/// JSON `matches` reply for a chunk fetch that couldn't be served now.
pub fn unserved(id: u32, start: u32) -> String {
    matches(id, None, start, None, None)
}

/// Serves a chunk fetch: `[]` on an `ACK` (the queue changed), `null` when
/// offline or MPD is unreachable.
pub async fn chunk(
    mpd: &Mpd,
    client: &mut Option<Client>,
    offline: bool,
    id: u32,
    filter: Filter,
    start: u32,
) -> String {
    if offline {
        return unserved(id, start);
    }
    let end = start.saturating_add(CHUNK);
    match execute(mpd, client, async move |c| {
        c.search_queue(&filter, start..end).await
    })
    .await
    {
        Ok(found) => matches(
            id,
            Some(found.queue_version),
            start,
            Some(&found.positions),
            None,
        ),
        Err(Error::Ack(ack)) => {
            tracing::debug!("search {id}: chunk {start}: {ack}");
            matches(id, None, start, Some(&[]), None)
        }
        Err(error) => {
            tracing::warn!("search {id}: chunk {start}: {error}");
            unserved(id, start)
        }
    }
}

/// A connection's running search: one MPD command per `step`.
pub struct Search {
    id: u32,
    /// Shared with each step's MPD call, which must own what it captures.
    filter: Arc<Filter>,
    /// The current song's position.
    from: Option<u32>,
    pub reply: mpsc::UnboundedSender<String>,
    /// Set by chunk 0 when the first match after `from` lies beyond it.
    probing: Option<Probing>,
}

struct Probing {
    version: u32,
    /// Resent with `first: 0` when no match follows `from`.
    chunk0: Vec<u32>,
    bracket: Bracket,
}

impl Search {
    pub fn new(
        id: u32,
        filter: Filter,
        from: Option<u32>,
        reply: mpsc::UnboundedSender<String>,
    ) -> Self {
        Search {
            id,
            filter: Arc::new(filter),
            from,
            reply,
            probing: None,
        }
    }

    /// Reads the next chunk and replies; false once the search is over.
    pub async fn step(&mut self, mpd: &Mpd, client: &mut Option<Client>, offline: bool) -> bool {
        let id = self.id;
        let chunk = self.probing.as_ref().map_or(0, |p| p.bracket.next());
        let start = chunk * CHUNK;
        if offline {
            let _ = self.reply.send(failed(id, "MPD offline"));
            return false;
        }
        let began = Instant::now();
        let filter = Arc::clone(&self.filter);
        let result = execute(mpd, client, async move |c| {
            c.search_queue(&filter, start..start + CHUNK).await
        })
        .await;
        tracing::debug!(
            "search {id}: window {start}:{}: {:.1} ms",
            start + CHUNK,
            began.elapsed().as_secs_f64() * 1e3
        );
        let send = |text: String| {
            // The client may have disconnected meanwhile.
            let _ = self.reply.send(text);
        };
        let found = match result {
            Ok(found) => found,
            // Chunk 0 ran the same query: a later `ACK` is the queue changing
            // (`Bad song index`).
            Err(Error::Ack(ack)) if self.probing.is_some() => {
                tracing::debug!("search {id}: {ack}");
                send(Message::SearchStale { id }.text());
                return false;
            }
            Err(Error::Ack(ack)) => {
                send(failed(id, &ack.message));
                return false;
            }
            Err(error) => {
                tracing::warn!("search {id}: {error}");
                send(failed(id, &error.to_string()));
                return false;
            }
        };
        let Some(probing) = &mut self.probing else {
            let more = first_chunk(id, self.from, &found, send);
            if more && let Some(from) = self.from {
                self.probing = Some(Probing {
                    version: found.queue_version,
                    bracket: Bracket::new(from, found.queue_length, &found.positions),
                    chunk0: found.positions,
                });
            }
            return more;
        };
        let version = probing.version;
        if found.queue_version != version {
            send(Message::SearchStale { id }.text());
            return false;
        }
        let positions = &found.positions;
        let reply =
            |start, positions, first| matches(id, Some(version), start, Some(positions), first);
        match probing.bracket.record(chunk, positions) {
            Probe::Next => {
                send(reply(start, positions, None));
                true
            }
            Probe::First(index) if index / CHUNK == chunk => {
                send(reply(start, positions, Some(index)));
                false
            }
            Probe::First(index) => {
                send(reply(start, positions, None));
                send(reply(index, probing.bracket.hi_positions(), Some(index)));
                false
            }
            Probe::Wrap => {
                send(reply(start, positions, None));
                send(reply(0, &probing.chunk0, Some(0)));
                false
            }
        }
    }
}

/// Replies with chunk 0, with `first` when it settles it; whether probing
/// must follow.
fn first_chunk(id: u32, from: Option<u32>, found: &QueueMatches, send: impl Fn(String)) -> bool {
    let positions = &found.positions;
    let first = match from {
        _ if positions.is_empty() => None,
        None => Some(0),
        Some(from) => match positions.partition_point(|&p| p <= from) {
            after if after < positions.len() => Some(after as u32),
            _ if positions.len() < CHUNK as usize => Some(0),
            _ => None,
        },
    };
    let version = Some(found.queue_version);
    send(matches(id, version, 0, Some(positions), first));
    !positions.is_empty() && first.is_none()
}

/// What a chunk told the bracket.
#[derive(Debug, PartialEq, Eq)]
enum Probe {
    /// Read `Bracket::next` next.
    Next,
    /// Index of the first match after `from`.
    First(u32),
    /// No match after `from`.
    Wrap,
}

/// What `Bracket::hi` is known to hold.
#[derive(Debug)]
enum Hi {
    /// Read: matches all after `from`.
    After(Vec<u32>),
    /// No matches.
    Empty,
    /// Unread: no matches, or all after `from`.
    Unknown,
}

/// Narrows down, in chunks, where the first match after `from` lies.
#[derive(Debug)]
struct Bracket {
    from: u32,
    /// Queue length.
    length: u32,
    /// Last chunk read whose matches all lie at or before `from`.
    lo: u32,
    /// Its last match's position.
    lo_last: u32,
    /// First chunk known to start after the first match after `from`, if any.
    hi: u32,
    hi_kind: Hi,
    /// Whether the last probe halved `hi − lo`; else the next one bisects.
    halved: bool,
}

impl Bracket {
    /// From a full chunk 0 whose matches all lie at or before `from`.
    fn new(from: u32, length: u32, chunk0: &[u32]) -> Self {
        let mut bracket = Bracket {
            from,
            length,
            lo: 0,
            lo_last: chunk0[CHUNK as usize - 1],
            hi: u32::MAX,
            hi_kind: Hi::Unknown,
            halved: true,
        };
        bracket.tighten();
        bracket
    }

    /// Lowers `hi` to what `lo` implies: positions grow by at least 1 per
    /// match, so from some index on every match lies after `from`, and from
    /// another none fits in the queue.
    fn tighten(&mut self) {
        let next = u64::from(self.lo + 1) * u64::from(CHUNK);
        let after = next + u64::from(self.from - self.lo_last);
        let none = next + u64::from(self.length - 1 - self.lo_last);
        let chunk = |index: u64| index.div_ceil(u64::from(CHUNK)) as u32;
        let (hi, kind) = if after < none {
            (chunk(after), Hi::Unknown)
        } else {
            (chunk(none), Hi::Empty)
        };
        // A chunk starting at or past the length has no matches.
        let kind = match kind {
            Hi::Unknown if u64::from(hi) * u64::from(CHUNK) >= u64::from(self.length) => Hi::Empty,
            kind => kind,
        };
        let better = matches!((&self.hi_kind, &kind), (Hi::Unknown, Hi::Empty));
        if hi < self.hi || (hi == self.hi && better) || self.hi <= self.lo {
            self.hi = hi;
            self.hi_kind = kind;
        }
    }

    /// Chunk to read next: interpolated, or bisected after a probe that
    /// didn't halve the gap.
    fn next(&self) -> u32 {
        if self.hi == self.lo + 1 {
            return self.hi;
        }
        let chunk = if self.halved {
            let next = f64::from(self.lo + 1) * f64::from(CHUNK);
            // Matches per position: between `lo` and `hi` when `hi` was read,
            // else up to `lo`.
            let density = match &self.hi_kind {
                Hi::After(positions) => {
                    let steps = f64::from(self.hi) * f64::from(CHUNK) - next + 1.0;
                    steps / f64::from(positions[0] - self.lo_last)
                }
                _ => next / (f64::from(self.lo_last) + 1.0),
            };
            let first = next + f64::from(self.from - self.lo_last) * density;
            (first / f64::from(CHUNK)) as u32
        } else {
            self.lo + (self.hi - self.lo) / 2
        };
        chunk.clamp(self.lo + 1, self.hi - 1)
    }

    /// Takes in chunk `chunk`'s `positions`, as asked by `next`.
    fn record(&mut self, chunk: u32, positions: &[u32]) -> Probe {
        let gap = self.hi - self.lo;
        match (positions.first(), positions.last()) {
            (None, _) => {
                self.hi = chunk;
                self.hi_kind = Hi::Empty;
            }
            (Some(&first), _) if first > self.from => {
                self.hi = chunk;
                self.hi_kind = Hi::After(positions.to_vec());
            }
            (_, Some(&last)) if last <= self.from => {
                if positions.len() < CHUNK as usize {
                    return Probe::Wrap;
                }
                self.lo = chunk;
                self.lo_last = last;
                self.tighten();
            }
            _ => {
                let after = positions.partition_point(|&p| p <= self.from);
                return Probe::First(chunk * CHUNK + after as u32);
            }
        }
        self.halved = 2 * (self.hi - self.lo) <= gap;
        if self.hi > self.lo + 1 {
            return Probe::Next;
        }
        match self.hi_kind {
            Hi::After(_) => Probe::First(self.hi * CHUNK),
            Hi::Empty => Probe::Wrap,
            Hi::Unknown => Probe::Next,
        }
    }

    /// `hi`'s matches once read.
    fn hi_positions(&self) -> &[u32] {
        match &self.hi_kind {
            Hi::After(positions) => positions,
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the bracket over the matches at `positions` (sorted) in a queue
    /// of `length`: the first match after `from` (`None`: wrap) and the
    /// chunks read after chunk 0.
    fn probe(positions: &[u32], length: u32, from: u32) -> (Option<u32>, u32) {
        let chunk = |c: u32| {
            let start = (c * CHUNK) as usize;
            assert!(start <= length as usize, "window starts past the queue");
            &positions[start.min(positions.len())..(start + CHUNK as usize).min(positions.len())]
        };
        let mut bracket = Bracket::new(from, length, chunk(0));
        let bound = 2 * (bracket.hi - bracket.lo).next_power_of_two().ilog2() + 1;
        let mut reads = 0;
        loop {
            let c = bracket.next();
            assert!(bracket.lo < c && c <= bracket.hi, "{c} outside {bracket:?}");
            reads += 1;
            assert!(reads <= bound, "over {bound} reads: {bracket:?}");
            match bracket.record(c, chunk(c)) {
                Probe::Next => {}
                Probe::First(index) => return (Some(index), reads),
                Probe::Wrap => return (None, reads),
            }
        }
    }

    fn expected(positions: &[u32], from: u32) -> Option<u32> {
        let after = positions.partition_point(|&p| p <= from);
        (after < positions.len()).then_some(after as u32)
    }

    /// xorshift: deterministic match sets without a dependency.
    fn random(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    #[test]
    fn finds_the_first_match_after_from_in_any_layout() {
        let length = 175_872;
        let mut seed = 0x2545_f491_4f6c_dd1d;
        let mut sets: Vec<Vec<u32>> = vec![
            (0..length).collect(),
            (0..length).step_by(3).collect(),
            (0..length).step_by(55).collect(),
            // Clustered at the start, then sparse.
            (0..2000).chain((2000..length).step_by(997)).collect(),
            // Sparse, then clustered at the end.
            (0..length - 3000)
                .step_by(331)
                .chain(length - 3000..length)
                .collect(),
            // One match past chunk 0.
            (0..500).chain([150_000]).collect(),
            // Exactly two full chunks.
            (0..1000).map(|i| i * 7).collect(),
        ];
        for density in [2, 20, 200] {
            sets.push(
                (0..length)
                    .filter(|_| random(&mut seed).is_multiple_of(density))
                    .collect(),
            );
        }
        for positions in sets.iter().filter(|p| p.len() >= CHUNK as usize) {
            let last_of_chunk0 = positions[CHUNK as usize - 1];
            let mut froms = vec![last_of_chunk0, length - 1, length + 10];
            for &p in [positions[positions.len() / 2], *positions.last().unwrap()].iter() {
                froms.extend([p - 1, p, p + 1]);
            }
            froms
                .extend((0..20).map(|_| {
                    last_of_chunk0 + random(&mut seed) as u32 % (length - last_of_chunk0)
                }));
            for from in froms.into_iter().filter(|&f| f >= last_of_chunk0) {
                let (first, _) = probe(positions, length, from);
                assert_eq!(first, expected(positions, from), "from {from}");
            }
        }
    }

    #[test]
    fn interpolation_reads_few_chunks_on_even_spreads() {
        let length = 175_872;
        // "love": 3,156 matches, one per ~56 positions.
        let love: Vec<u32> = (0..3156).map(|i| i * 55 + i % 7).collect();
        // "the": 47,646, one per ~3.7.
        let the: Vec<u32> = (0..47_646u32).map(|i| i * 37 / 10).collect();
        for (positions, from) in [
            (&love, 30_000),
            (&love, 90_000),
            (&love, 170_000),
            (&the, 100_000),
        ] {
            let (first, reads) = probe(positions, length, from);
            assert_eq!(first, expected(positions, from));
            assert!(reads <= 2, "{reads} reads from {from}");
        }
    }
}
