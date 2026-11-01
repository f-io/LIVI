// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub(super) const NAL_LENGTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: u32,
    pub timescale: u32,
    pub avcc: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub(super) default_duration: u32,
    pub(super) default_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub data: Vec<u8>,
    pub duration: u32,
}

pub(super) fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |acc, b| (acc << 8) | u64::from(*b))
}

pub(super) fn u32_at(buf: &[u8], at: usize) -> Option<u32> {
    Some(be(buf.get(at..at + 4)?) as u32)
}

pub(super) fn boxes(buf: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8], usize)> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let size = u32_at(buf, at)? as usize;
        let kind: [u8; 4] = buf.get(at + 4..at + 8)?.try_into().ok()?;
        let (head, size) = match size {
            0 => (8, buf.len() - at),
            1 => (16, usize::try_from(be(buf.get(at + 8..at + 16)?)).ok()?),
            n => (8, n),
        };
        let body = buf.get(at + head..at.checked_add(size)?)?;
        let start = at;
        at += size;
        Some((kind, body, start))
    })
}

pub(super) fn child<'a>(buf: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(buf)
        .find(|(k, _, _)| k == kind)
        .map(|(_, body, _)| body)
}

pub(super) fn path<'a>(buf: &'a [u8], kinds: &[&[u8; 4]]) -> Option<&'a [u8]> {
    kinds.iter().try_fold(buf, |at, kind| child(at, kind))
}

pub(super) fn video_track(trak: &[u8]) -> Option<Track> {
    let mdia = child(trak, b"mdia")?;
    if child(mdia, b"hdlr")?.get(8..12)? != b"vide" {
        return None;
    }
    let mdhd = child(mdia, b"mdhd")?;
    let timescale = if mdhd.first()? == &1 {
        u32_at(mdhd, 20)?
    } else {
        u32_at(mdhd, 12)?
    };
    let stsd = path(mdia, &[b"minf", b"stbl", b"stsd"])?;
    let (kind, entry, _) = boxes(stsd.get(8..)?).next()?;
    if &kind != b"avc1" && &kind != b"avc3" {
        return None;
    }
    let width = be(entry.get(24..26)?) as u32;
    let height = be(entry.get(26..28)?) as u32;
    let avcc = child(entry.get(78..)?, b"avcC")?.to_vec();
    if usize::from(avcc.get(4)? & 3) + 1 != NAL_LENGTH {
        return None;
    }
    let tkhd = child(trak, b"tkhd")?;
    let id = if tkhd.first()? == &1 {
        u32_at(tkhd, 20)?
    } else {
        u32_at(tkhd, 12)?
    };
    Some(Track {
        id,
        timescale,
        avcc,
        width,
        height,
        default_duration: 0,
        default_size: 0,
    })
}

pub fn read_init(init: &[u8]) -> Option<Track> {
    let moov = child(init, b"moov")?;
    let mut track = boxes(moov)
        .filter(|(k, _, _)| k == b"trak")
        .find_map(|(_, trak, _)| video_track(trak))?;
    if let Some(mvex) = child(moov, b"mvex") {
        for (_, trex, _) in boxes(mvex).filter(|(k, _, _)| k == b"trex") {
            if u32_at(trex, 4) == Some(track.id) {
                track.default_duration = u32_at(trex, 12).unwrap_or(0);
                track.default_size = u32_at(trex, 16).unwrap_or(0);
            }
        }
    }
    Some(track)
}

pub(super) const TFHD_BASE_OFFSET: u32 = 0x1;
pub(super) const TFHD_DESCRIPTION: u32 = 0x2;
pub(super) const TFHD_DURATION: u32 = 0x8;
pub(super) const TFHD_SIZE: u32 = 0x10;
pub(super) const TRUN_DATA_OFFSET: u32 = 0x1;
pub(super) const TRUN_FIRST_FLAGS: u32 = 0x4;
pub(super) const TRUN_DURATION: u32 = 0x100;
pub(super) const TRUN_SIZE: u32 = 0x200;
pub(super) const TRUN_FLAGS: u32 = 0x400;
pub(super) const TRUN_CTO: u32 = 0x800;

pub fn read_segment(segment: &[u8], track: &Track) -> Vec<Sample> {
    let mut samples = Vec::new();
    for (kind, moof, moof_at) in boxes(segment) {
        if &kind != b"moof" {
            continue;
        }
        for (_, traf, _) in boxes(moof).filter(|(k, _, _)| k == b"traf") {
            read_traf(segment, moof_at, traf, track, &mut samples);
        }
    }
    samples
}

pub(super) fn read_traf(
    segment: &[u8],
    moof_at: usize,
    traf: &[u8],
    track: &Track,
    out: &mut Vec<Sample>,
) {
    let Some(tfhd) = child(traf, b"tfhd") else {
        return;
    };
    let (Some(flags), Some(id)) = (u32_at(tfhd, 0), u32_at(tfhd, 4)) else {
        return;
    };
    if id != track.id {
        return;
    }
    let flags = flags & 0x00FF_FFFF;
    let mut at = 8;
    let mut base = moof_at;
    if flags & TFHD_BASE_OFFSET != 0 {
        base = tfhd.get(at..at + 8).map_or(moof_at, |b| be(b) as usize);
        at += 8;
    }
    if flags & TFHD_DESCRIPTION != 0 {
        at += 4;
    }
    let mut duration = track.default_duration;
    if flags & TFHD_DURATION != 0 {
        duration = u32_at(tfhd, at).unwrap_or(duration);
        at += 4;
    }
    let size = if flags & TFHD_SIZE != 0 {
        u32_at(tfhd, at)
    } else {
        None
    };
    let size = size.unwrap_or(track.default_size);
    let mut next = base;
    for (_, trun, _) in boxes(traf).filter(|(k, _, _)| k == b"trun") {
        next = read_trun(segment, base, next, trun, (duration, size), out);
    }
}

pub(super) fn read_trun(
    segment: &[u8],
    base: usize,
    mut data: usize,
    trun: &[u8],
    (default_duration, default_size): (u32, u32),
    out: &mut Vec<Sample>,
) -> usize {
    let (Some(flags), Some(count)) = (u32_at(trun, 0), u32_at(trun, 4)) else {
        return data;
    };
    let mut at = 8;
    if flags & TRUN_DATA_OFFSET != 0 {
        let Some(offset) = u32_at(trun, at) else {
            return data;
        };
        data = base.wrapping_add_signed(offset as i32 as isize);
        at += 4;
    }
    if flags & TRUN_FIRST_FLAGS != 0 {
        at += 4;
    }
    for _ in 0..count {
        let mut field = |bit: u32, default: u32| {
            if flags & bit == 0 {
                return Some(default);
            }
            let v = u32_at(trun, at);
            at += 4;
            v
        };
        let (Some(duration), Some(size)) = (
            field(TRUN_DURATION, default_duration),
            field(TRUN_SIZE, default_size),
        ) else {
            return data;
        };
        let _ = field(TRUN_FLAGS, 0);
        let _ = field(TRUN_CTO, 0);
        let Some(bytes) = segment.get(data..data + size as usize) else {
            return data;
        };
        out.push(Sample {
            data: bytes.to_vec(),
            duration,
        });
        data += size as usize;
    }
    data
}
