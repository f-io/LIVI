// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Playlist {
    pub init: Option<String>,
    pub first: u64,
    pub segments: Vec<String>,
    pub discontinuity: u64,
    pub block_reload: bool,
}

pub fn read_playlist(text: &str) -> Playlist {
    let mut list = Playlist::default();
    for line in text.lines().map(str::trim) {
        if let Some(seq) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            list.first = seq.parse().unwrap_or(0);
        } else if let Some(seq) = line.strip_prefix("#EXT-X-DISCONTINUITY-SEQUENCE:") {
            list.discontinuity = seq.parse().unwrap_or(0);
        } else if let Some(control) = line.strip_prefix("#EXT-X-SERVER-CONTROL:") {
            list.block_reload = control
                .split(',')
                .any(|a| a.trim() == "CAN-BLOCK-RELOAD=YES");
        } else if let Some(map) = line.strip_prefix("#EXT-X-MAP:") {
            list.init = map
                .split("URI=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .map(str::to_string);
        } else if !line.is_empty() && !line.starts_with('#') {
            list.segments.push(line.to_string());
        }
    }
    list
}

pub(crate) fn resolve(base: &str, uri: &str) -> String {
    if uri.contains("://") {
        return uri.to_string();
    }
    if uri.starts_with("//") {
        let scheme = base.split_once("://").map_or("http", |(scheme, _)| scheme);
        return format!("{scheme}:{uri}");
    }
    let origin_end = base.find("://").map_or(0, |at| at + 3);
    let host_end = base[origin_end..]
        .find('/')
        .map_or(base.len(), |at| origin_end + at);
    if uri.starts_with('/') {
        return format!("{}{uri}", &base[..host_end]);
    }
    let path = base.split(['?', '#']).next().unwrap_or(base);
    let dir_end = path
        .rfind('/')
        .filter(|&at| at >= host_end)
        .map_or(path.len(), |at| at + 1);
    let sep = if dir_end == path.len() && !path.ends_with('/') {
        "/"
    } else {
        ""
    };
    format!("{}{sep}{uri}", &path[..dir_end])
}
