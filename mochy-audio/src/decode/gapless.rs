//! Gapless encoder-delay accounting for MP4/AAC — ours to own.
//!
//! Symphonia 0.6 does not expose gapless metadata; the upstream app never
//! had to care because MediaCodec/MediaExtractor honored MP4 edit lists
//! itself (`StreamPcmDecoder.kt:38-41` notes the decoder's timestamps
//! "start below zero" for encoder padding — "which is what the player
//! skips too"). The desktop port does the skipping itself:
//! [`parse_mp4_gapless`] walks the box tree for the audio track's edit
//! list and returns the sample-accurate trim, which
//! [`super::TrackDecoder`] applies to every decoded block.
//!
//! What is read:
//! - `moov.trak.mdia.hdlr` — the track with handler `soun` is the audio
//!   one;
//! - `moov.trak.mdia.mdhd` — the media timescale (for audio it is the
//!   sample rate) and the track's total media duration;
//! - `moov.trak.edts.elst` — the edit list. The first non-empty edit's
//!   `media_time` is the encoder priming (in media samples — the samples
//!   to skip at the start); when the track's media duration exceeds
//!   priming + the edit span, the excess is the encoder padding trimmed
//!   from the end.
//!
//! The `iTunSMBX` freeform tag is deliberately not parsed: the edit list
//! is the mechanism YouTube's MP4 muxer writes, and inventing a reading
//! of Apple's private tag would be worse than omitting it (recorded in
//! HANDOFF ## Phase 08 decisions).

use std::io::{Read, Seek, SeekFrom};

/// The sample-accurate gapless trim for one MP4 audio track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GaplessInfo {
    /// Encoder priming: samples to skip at the start of the decoded
    /// stream (the edit list's `media_time`).
    pub priming_samples: u64,
    /// Encoder padding: samples to skip at the end. `None` when the
    /// container doesn't say (the track plays to its end).
    pub padding_samples: Option<u64>,
    /// The media timescale the two counts are in (for audio tracks,
    /// typically the sample rate).
    pub timescale: u32,
}

/// Refuses absurd box walks: a `moov` larger than this is skipped.
const MAX_MOOV_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
struct BoxHeader {
    kind: [u8; 4],
    content_start: u64,
    /// Bytes after the header (the box's payload size).
    content_len: u64,
}

impl BoxHeader {
    /// The offset of the box that follows this one.
    fn next(&self) -> u64 {
        self.content_start + self.content_len
    }
}

fn read_box_header<R: Read>(
    reader: &mut R,
    end: u64,
    at: u64,
) -> std::io::Result<Option<BoxHeader>> {
    if at + 8 > end {
        return Ok(None);
    }
    let mut header = [0u8; 8];
    reader.read_exact(&mut header)?;
    let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
    let kind = [header[4], header[5], header[6], header[7]];
    let (content_len, header_len) = if size32 == 1 {
        // 64-bit largesize box.
        if at + 16 > end {
            return Ok(None);
        }
        let mut largesize = [0u8; 8];
        reader.read_exact(&mut largesize)?;
        (u64::from_be_bytes(largesize), 16u64)
    } else if size32 == 0 {
        // To-the-end box.
        (end - at, 8u64)
    } else {
        (size32, 8u64)
    };
    if content_len < header_len || at + content_len > end {
        return Ok(None);
    }
    Ok(Some(BoxHeader {
        kind,
        content_start: at + header_len,
        content_len: content_len - header_len,
    }))
}

fn read_exact_at<R: Read + Seek>(reader: &mut R, at: u64, buf: &mut [u8]) -> std::io::Result<()> {
    reader.seek(SeekFrom::Start(at))?;
    reader.read_exact(buf)
}

/// Walks an MP4 for the audio track's gapless trim. Returns `None` when
/// anything about the container doesn't say (no edit list, video-only,
/// not an MP4 at all) — the caller plays the track untrimmed.
pub fn parse_mp4_gapless<R: Read + Seek>(reader: &mut R) -> std::io::Result<Option<GaplessInfo>> {
    let file_len = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut at = 0u64;
    while let Some(header) = read_box_header(reader, file_len, at)? {
        if &header.kind == b"moov" {
            if header.content_len > MAX_MOOV_BYTES {
                return Ok(None);
            }
            return parse_moov(reader, header.content_start, header.content_len);
        }
        at = header.next();
        reader.seek(SeekFrom::Start(at))?;
    }
    Ok(None)
}

fn parse_moov<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    len: u64,
) -> std::io::Result<Option<GaplessInfo>> {
    let end = start + len;
    let mut at = start;
    while let Some(header) = read_box_header(reader, end, at)? {
        if &header.kind == b"trak"
            && let Some(info) = parse_trak(reader, header.content_start, header.content_len)?
        {
            return Ok(Some(info));
        }
        at = header.next();
        reader.seek(SeekFrom::Start(at))?;
    }
    Ok(None)
}

struct TrakFacts {
    is_audio: bool,
    timescale: u32,
    media_duration: Option<u64>,
    edit: Option<(u64, i64)>, // (segment_duration_movie_units, media_time)
}

fn parse_trak<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    len: u64,
) -> std::io::Result<Option<GaplessInfo>> {
    let end = start + len;
    let mut at = start;
    let mut facts = TrakFacts {
        is_audio: false,
        timescale: 0,
        media_duration: None,
        edit: None,
    };
    while let Some(header) = read_box_header(reader, end, at)? {
        match &header.kind {
            b"mdia" => parse_mdia(reader, &header, &mut facts)?,
            b"edts" => parse_edts(reader, &header, &mut facts)?,
            _ => {}
        }
        at = header.next();
        reader.seek(SeekFrom::Start(at))?;
    }
    let TrakFacts {
        is_audio,
        timescale,
        media_duration,
        edit,
    } = facts;
    if !is_audio || timescale == 0 {
        return Ok(None);
    }
    let Some((segment_duration, media_time)) = edit else {
        // An audio track with no edit list: nothing to trim.
        return Ok(None);
    };
    if media_time <= 0 {
        // media_time 0 = no priming; -1 = an empty edit (a leading delay),
        // which carries no trim for us.
        return Ok(None);
    }
    let priming = media_time as u64;
    // The end trim: when the track's media holds more than priming + the
    // edit's span, the excess is the encoder padding (the AAC stream is
    // priming + real + padding; the edit list plays only the real part).
    // The segment duration is in the MOVIE timescale, which we don't read,
    // so this only fires for the common shape where it under-counts the
    // media — the honest `None` covers everything else.
    let padding = media_duration
        .map(|duration| {
            duration
                .saturating_sub(priming)
                .saturating_sub(segment_duration)
        })
        .filter(|p| *p > 0);
    Ok(Some(GaplessInfo {
        priming_samples: priming,
        padding_samples: padding,
        timescale,
    }))
}

fn parse_mdia<R: Read + Seek>(
    reader: &mut R,
    mdia: &BoxHeader,
    facts: &mut TrakFacts,
) -> std::io::Result<()> {
    let end = mdia.content_start + mdia.content_len;
    let mut at = mdia.content_start;
    while let Some(sub) = read_box_header(reader, end, at)? {
        match &sub.kind {
            b"mdhd" => {
                // version(1) flags(3) then v0: dates(8) timescale(4)
                // duration(4); v1: dates(16) timescale(4) duration(8).
                let mut version = [0u8; 1];
                read_exact_at(reader, sub.content_start, &mut version)?;
                if version[0] == 1 {
                    let mut buf = [0u8; 32];
                    read_exact_at(reader, sub.content_start, &mut buf)?;
                    facts.timescale = u32::from_be_bytes([buf[20], buf[21], buf[22], buf[23]]);
                    facts.media_duration = Some(u64::from_be_bytes([
                        buf[24], buf[25], buf[26], buf[27], buf[28], buf[29], buf[30], buf[31],
                    ]));
                } else {
                    let mut buf = [0u8; 20];
                    read_exact_at(reader, sub.content_start, &mut buf)?;
                    facts.timescale = u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]);
                    facts.media_duration =
                        Some(u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]) as u64);
                }
            }
            b"hdlr" => {
                // version/flags(4) + predefined(4) + handler type(4).
                let mut buf = [0u8; 12];
                read_exact_at(reader, sub.content_start, &mut buf)?;
                if &buf[8..12] == b"soun" {
                    facts.is_audio = true;
                }
            }
            _ => {}
        }
        at = sub.next();
        reader.seek(SeekFrom::Start(at))?;
    }
    Ok(())
}

fn parse_edts<R: Read + Seek>(
    reader: &mut R,
    edts: &BoxHeader,
    facts: &mut TrakFacts,
) -> std::io::Result<()> {
    let end = edts.content_start + edts.content_len;
    let mut at = edts.content_start;
    while let Some(sub) = read_box_header(reader, end, at)? {
        if &sub.kind == b"elst" {
            facts.edit = parse_elst(reader, sub.content_start, sub.content_len)?;
        }
        at = sub.next();
        reader.seek(SeekFrom::Start(at))?;
    }
    Ok(())
}

/// Parses one `elst` box: the first non-empty edit's
/// `(segment_duration, media_time)`.
fn parse_elst(
    reader: &mut (impl Read + Seek),
    start: u64,
    len: u64,
) -> std::io::Result<Option<(u64, i64)>> {
    let mut head = [0u8; 8];
    read_exact_at(reader, start, &mut head)?;
    let version = head[0];
    let entries = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as u64;
    let entry_size: u64 = if version == 1 { 20 } else { 12 };
    if entries == 0 || entries > 64 || len < 8 + entries * entry_size {
        return Ok(None);
    }
    let mut best: Option<(u64, i64)> = None;
    for i in 0..entries {
        let mut entry = [0u8; 20];
        read_exact_at(reader, start + 8 + i * entry_size, &mut entry)?;
        let (segment, media_time) = if version == 1 {
            (
                u64::from_be_bytes([
                    entry[0], entry[1], entry[2], entry[3], entry[4], entry[5], entry[6], entry[7],
                ]),
                i64::from_be_bytes([
                    entry[8], entry[9], entry[10], entry[11], entry[12], entry[13], entry[14],
                    entry[15],
                ]),
            )
        } else {
            (
                u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]) as u64,
                i32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]) as i64,
            )
        };
        // The first non-empty edit (media_time != -1) carries the priming.
        if media_time != -1 {
            best = Some((segment, media_time));
            break;
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn box_bytes(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&((payload.len() as u32 + 8).to_be_bytes()));
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    fn hdlr(handler: &[u8; 4]) -> Vec<u8> {
        // version/flags(4) + predefined(4) + handler(4) + reserved(12) + name(1)
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0, 0, 0, 0]);
        payload.extend_from_slice(&[0, 0, 0, 0]);
        payload.extend_from_slice(handler);
        payload.extend_from_slice(&[0u8; 12]);
        payload.push(0);
        box_bytes(b"hdlr", &payload)
    }

    fn mdhd(timescale: u32, duration: u64) -> Vec<u8> {
        // v0 mdhd: version+flags(4) dates(8) timescale(4) duration(4)
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0, 0, 0, 0]);
        payload.extend_from_slice(&[0u8; 8]);
        payload.extend_from_slice(&timescale.to_be_bytes());
        payload.extend_from_slice(&(duration as u32).to_be_bytes());
        box_bytes(b"mdhd", &payload)
    }

    fn elst(entries: &[(u64, i64)]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0, 0, 0, 0]); // version 0 + flags
        payload.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for (segment, media) in entries {
            payload.extend_from_slice(&(*segment as u32).to_be_bytes());
            payload.extend_from_slice(&(*media as i32).to_be_bytes());
            payload.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate 1.0
        }
        box_bytes(b"elst", &payload)
    }

    fn audio_trak(timescale: u32, duration: u64, edits: Option<Vec<u8>>) -> Vec<u8> {
        let mut mdia = mdhd(timescale, duration);
        mdia.extend(hdlr(b"soun"));
        mdia.extend(box_bytes(b"minf", &[]));
        let mdia_box = box_bytes(b"mdia", &mdia);
        match edits {
            Some(edits) => {
                let mut with_edits = box_bytes(b"edts", &edits);
                with_edits.extend_from_slice(&mdia_box);
                box_bytes(b"trak", &with_edits)
            }
            None => box_bytes(b"trak", &mdia_box),
        }
    }

    fn mp4(traks: Vec<Vec<u8>>) -> Vec<u8> {
        let mut moov = box_bytes(b"mvhd", &[]);
        for trak in traks {
            moov.extend_from_slice(&trak);
        }
        let mut file = box_bytes(b"ftyp", b"M4A ");
        file.extend(box_bytes(b"moov", &moov));
        file
    }

    #[test]
    fn reads_priming_and_padding() {
        // A 44.1 kHz track: 1024 samples priming, 1964 media duration,
        // 300 edit span → 640 padding.
        let file = mp4(vec![audio_trak(44_100, 1964, Some(elst(&[(300, 1024)])))]);
        let mut cursor = Cursor::new(file);
        let info = parse_mp4_gapless(&mut cursor).unwrap().unwrap();
        assert_eq!(info.priming_samples, 1024);
        assert_eq!(info.padding_samples, Some(640));
        assert_eq!(info.timescale, 44_100);
    }

    #[test]
    fn no_edit_list_means_no_trim() {
        let file = mp4(vec![audio_trak(48_000, 5000, None)]);
        let mut cursor = Cursor::new(file);
        assert!(parse_mp4_gapless(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn empty_edit_is_skipped_for_the_real_one() {
        // media_time -1 = empty edit (leading silence in movie time); the
        // real edit after it carries the priming.
        let file = mp4(vec![audio_trak(
            48_000,
            4096,
            Some(elst(&[(0, -1), (4096, 2048)])),
        )]);
        let mut cursor = Cursor::new(file);
        let info = parse_mp4_gapless(&mut cursor).unwrap().unwrap();
        assert_eq!(info.priming_samples, 2048);
        // 4096 - 2048 - 4096 saturates to 0 → filtered to None.
        assert_eq!(info.padding_samples, None);
    }

    #[test]
    fn video_tracks_are_ignored() {
        let mut mdia = mdhd(900, 1000);
        mdia.extend(hdlr(b"vide"));
        let mdia_box = box_bytes(b"mdia", &mdia);
        let mut with_edits = elst(&[(500, 100)]);
        with_edits.extend_from_slice(&mdia_box);
        let file = mp4(vec![box_bytes(b"trak", &with_edits)]);
        let mut cursor = Cursor::new(file);
        assert!(parse_mp4_gapless(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn first_audio_trak_wins_over_a_video_one() {
        // Two traks: video first, audio second — the audio edit list is
        // the one that counts.
        let mut mdia = mdhd(900, 1000);
        mdia.extend(hdlr(b"vide"));
        let video = box_bytes(b"trak", &box_bytes(b"mdia", &mdia));
        let audio = audio_trak(44_100, 3000, Some(elst(&[(100, 1024)])));
        let file = mp4(vec![video, audio]);
        let mut cursor = Cursor::new(file);
        let info = parse_mp4_gapless(&mut cursor).unwrap().unwrap();
        assert_eq!(info.timescale, 44_100);
        assert_eq!(info.priming_samples, 1024);
    }

    #[test]
    fn not_an_mp4_is_none_not_an_error() {
        let mut cursor = Cursor::new(vec![0u8; 128]);
        assert!(parse_mp4_gapless(&mut cursor).unwrap().is_none());
        let mut empty = Cursor::new(Vec::new());
        assert!(parse_mp4_gapless(&mut empty).unwrap().is_none());
    }

    #[test]
    fn v1_elst_entries_parse() {
        // version 1: 64-bit segment duration and media time.
        let mut payload = Vec::new();
        payload.extend_from_slice(&[1, 0, 0, 0]); // version 1 + flags
        payload.extend_from_slice(&1u32.to_be_bytes());
        payload.extend_from_slice(&100_000u64.to_be_bytes());
        payload.extend_from_slice(&2112i64.to_be_bytes());
        payload.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        let elst = box_bytes(b"elst", &payload);
        let file = mp4(vec![audio_trak(44_100, 44_100, Some(elst))]);
        let mut cursor = Cursor::new(file);
        let info = parse_mp4_gapless(&mut cursor).unwrap().unwrap();
        assert_eq!(info.priming_samples, 2112);
    }

    #[test]
    fn v1_mdhd_duration_parses() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[1, 0, 0, 0]); // version 1
        payload.extend_from_slice(&[0u8; 16]); // dates
        payload.extend_from_slice(&44_100u32.to_be_bytes());
        payload.extend_from_slice(&441_000u64.to_be_bytes());
        let mdia = box_bytes(b"mdhd", &payload);
        let mdia_box = {
            let mut m = mdia;
            m.extend(hdlr(b"soun"));
            box_bytes(b"mdia", &m)
        };
        let file = mp4(vec![box_bytes(b"trak", &mdia_box)]);
        let mut cursor = Cursor::new(file);
        // No edit list → None, but the walk must not misread the v1 box
        // sizes and wander into garbage.
        assert!(parse_mp4_gapless(&mut cursor).unwrap().is_none());
    }
}
