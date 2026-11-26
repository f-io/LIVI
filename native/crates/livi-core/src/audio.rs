// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg_attr(target_os = "android", path = "audio/audio_devices/android.rs")]
pub mod audio_devices;
pub mod heard;
pub mod host_output;
#[cfg(target_os = "ios")]
pub mod ios_volume;
pub mod spectrum;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
pub mod system_volume;
