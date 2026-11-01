// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod auth_setup;
pub mod bplist;
pub mod control_cipher;
pub mod crypto;
pub mod helper_sock;
pub mod hid;
pub mod hls;
pub mod hls_gateway;
pub mod iap_tunnel;
pub mod identity;
pub mod image_archive;
pub mod info;
pub mod keep_alive;
pub mod manager;
pub mod media;
pub mod mfi4;
pub mod net;
pub mod opack;
pub mod pair_setup;
pub mod pair_verify;
pub mod pairings;
pub mod rtsp;
pub mod srp;
pub mod stack;
pub mod timing;
pub mod timing_sync;
pub mod tlv8;
pub mod ui_sync;
pub mod vehicle_state;
pub mod video_playback;
pub mod video_proxy;

#[cfg(test)]
mod test_tls;
#[cfg(test)]
mod ts_vectors;
