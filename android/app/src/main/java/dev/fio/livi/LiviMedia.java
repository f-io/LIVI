// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

package dev.fio.livi;

import android.content.Context;
import android.content.Intent;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.Build;
import android.view.KeyEvent;

final class LiviMedia {
    private static final int PLAY_PAUSE = 0;
    private static final int PLAY = 1;
    private static final int PAUSE = 2;
    private static final int NEXT = 3;
    private static final int PREV = 4;

    private LiviMedia() {}

    static MediaSession start(Context context) {
        MediaSession session = new MediaSession(context, "LIVI");
        session.setCallback(new MediaSession.Callback() {
            @Override
            public void onPlay() {
                LiviActivity.media(PLAY);
            }

            @Override
            public void onPause() {
                LiviActivity.media(PAUSE);
            }

            @Override
            public void onSkipToNext() {
                LiviActivity.media(NEXT);
            }

            @Override
            public void onSkipToPrevious() {
                LiviActivity.media(PREV);
            }

            @Override
            public boolean onMediaButtonEvent(Intent intent) {
                KeyEvent event = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
                        ? intent.getParcelableExtra(Intent.EXTRA_KEY_EVENT, KeyEvent.class)
                        : intent.getParcelableExtra(Intent.EXTRA_KEY_EVENT);
                if (event != null
                        && event.getAction() == KeyEvent.ACTION_DOWN
                        && event.getRepeatCount() == 0
                        && (event.getKeyCode() == KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE
                                || event.getKeyCode() == KeyEvent.KEYCODE_HEADSETHOOK)) {
                    LiviActivity.media(PLAY_PAUSE);
                    return true;
                }
                return super.onMediaButtonEvent(intent);
            }
        });
        session.setPlaybackState(state(false));
        session.setActive(true);
        return session;
    }

    static void setPlaying(MediaSession session, boolean playing) {
        session.setPlaybackState(state(playing));
    }

    private static PlaybackState state(boolean playing) {
        return new PlaybackState.Builder()
                .setActions(PlaybackState.ACTION_PLAY
                        | PlaybackState.ACTION_PAUSE
                        | PlaybackState.ACTION_PLAY_PAUSE
                        | PlaybackState.ACTION_SKIP_TO_NEXT
                        | PlaybackState.ACTION_SKIP_TO_PREVIOUS)
                .setState(playing ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED,
                        PlaybackState.PLAYBACK_POSITION_UNKNOWN, 1f)
                .build();
    }
}
