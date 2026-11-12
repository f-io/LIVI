// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod airplay;
pub(crate) mod auth_setup;
pub(crate) mod bplist;
pub(crate) mod control_cipher;
pub mod helper_sock;
pub mod hid;
pub(crate) mod hls;
pub(crate) mod hls_gateway;
pub(crate) mod iap_tunnel;
pub mod identity;
pub(crate) mod image_archive;
pub mod info;
pub(crate) mod keep_alive;
pub mod manager;
pub mod media;
pub(crate) mod mfi4;
pub mod net;
pub(crate) mod opack;
pub(crate) mod pair_setup;
pub(crate) mod pair_verify;
pub mod pairings;
pub(crate) mod rtsp;
pub(crate) mod session_keys;
pub(crate) mod srp;
pub mod stack;
pub(crate) mod timing;
pub(crate) mod timing_sync;
pub(crate) mod tlv8;
pub mod ui_sync;
pub mod vehicle_state;
pub(crate) mod video_playback;
pub(crate) mod video_proxy;

#[cfg(test)]
mod test_tls;
#[cfg(test)]
mod ts_vectors;
