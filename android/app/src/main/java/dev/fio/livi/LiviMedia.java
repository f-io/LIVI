// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

package dev.fio.livi;

import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.Build;
import android.view.KeyEvent;
import java.nio.ByteBuffer;

final class LiviMedia {
    private static final int PLAY_PAUSE = 0;
    private static final int PLAY = 1;
    private static final int PAUSE = 2;
    private static final int NEXT = 3;
    private static final int PREV = 4;

    private final MediaSession session;
    private final AudioManager audio;
    private final AudioFocusRequest focus;
    private boolean held;

    LiviMedia(Context context) {
        audio = context.getSystemService(AudioManager.class);
        focus = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                .setAudioAttributes(new AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build())
                .setOnAudioFocusChangeListener(change -> held = change == AudioManager.AUDIOFOCUS_GAIN)
                .build();
        session = new MediaSession(context, "LIVI");
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
    }

    void setPlaying(boolean playing) {
        if (playing && !held && audio != null) {
            held = audio.requestAudioFocus(focus) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED;
        }
        session.setPlaybackState(state(playing));
    }

    void setMetadata(String title, String artist, String album, long durationMs, byte[] art, int width, int height) {
        if (title.isEmpty() && artist.isEmpty() && album.isEmpty()) {
            session.setMetadata(null);
            return;
        }
        MediaMetadata.Builder meta = new MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, artist)
                .putString(MediaMetadata.METADATA_KEY_ALBUM, album);
        if (durationMs > 0) {
            meta.putLong(MediaMetadata.METADATA_KEY_DURATION, durationMs);
        }
        if (art != null && width > 0 && height > 0 && art.length == width * height * 4) {
            Bitmap cover = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888);
            cover.copyPixelsFromBuffer(ByteBuffer.wrap(art));
            meta.putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, cover);
        }
        session.setMetadata(meta.build());
    }

    void release() {
        if (audio != null) {
            audio.abandonAudioFocusRequest(focus);
        }
        session.release();
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
