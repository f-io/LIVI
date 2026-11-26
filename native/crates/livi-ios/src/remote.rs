// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::ptr::NonNull;

use block2::RcBlock;
use objc2_avf_audio::{AVAudioSession, AVAudioSessionCategoryPlayback};
use objc2_media_player::{
    MPNowPlayingInfoCenter, MPNowPlayingPlaybackState, MPRemoteCommand, MPRemoteCommandCenter,
    MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};

pub fn start() {
    activate();
    commands();
    livi_ui::set_media_sink(state);
    state(false);
}

fn activate() {
    // SAFETY: the shared audio session, set to the playback category and activated.
    unsafe {
        let session = AVAudioSession::sharedInstance();
        if let Some(category) = AVAudioSessionCategoryPlayback {
            let _ = session.setCategory_error(category);
        }
        let _ = session.setActive_error(true);
    }
}

fn commands() {
    // SAFETY: the shared remote command center and its standard transport commands.
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        bind(&center.togglePlayPauseCommand(), 0);
        bind(&center.playCommand(), 1);
        bind(&center.pauseCommand(), 2);
        bind(&center.nextTrackCommand(), 3);
        bind(&center.previousTrackCommand(), 4);
    }
}

fn bind(command: &MPRemoteCommand, code: i32) {
    let handler = RcBlock::new(move |_event: NonNull<MPRemoteCommandEvent>| {
        let _ = slint::invoke_from_event_loop(move || livi_ui::media(code));
        MPRemoteCommandHandlerStatus::Success
    });
    // SAFETY: enabling the command and registering a handler block the center copies.
    unsafe {
        command.setEnabled(true);
        let _ = command.addTargetWithHandler(&handler);
    }
}

fn state(playing: bool) {
    let playback = if playing {
        MPNowPlayingPlaybackState::Playing
    } else {
        MPNowPlayingPlaybackState::Paused
    };
    // SAFETY: the default now-playing center, given the current playback state.
    unsafe {
        MPNowPlayingInfoCenter::defaultCenter().setPlaybackState(playback);
    }
}
