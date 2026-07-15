from dataclasses import dataclass
from enum import IntEnum
from typing import Annotated, Optional

from iap2.control_session_message import csm, NoneLike, Uint8, Uint32, Uint64


class PlaybackStatus(IntEnum):
    STOPPED = 0
    PLAYING = 1
    PAUSED = 2
    SEEK_FORWARD = 3
    SEEK_BACKWARD = 4


@dataclass
class StartMediaItemAttributes:
    persistent_id: Annotated[NoneLike, 0] = None
    title: Annotated[NoneLike, 1] = None
    duration_ms: Annotated[NoneLike, 4] = None
    album: Annotated[NoneLike, 6] = None
    artist: Annotated[NoneLike, 12] = None
    album_artist: Annotated[NoneLike, 14] = None
    genre: Annotated[NoneLike, 16] = None
    artwork: Annotated[NoneLike, 26] = None


@dataclass
class StartPlaybackAttributes:
    status: Annotated[NoneLike, 0] = None
    elapsed_ms: Annotated[NoneLike, 1] = None
    app_name: Annotated[NoneLike, 7] = None
    app_bundle_id: Annotated[NoneLike, 16] = None


@csm(0x5000)
@dataclass
class StartNowPlayingUpdates:
    media_item_attributes: Annotated[Optional[StartMediaItemAttributes], 0] = None
    playback_attributes: Annotated[Optional[StartPlaybackAttributes], 1] = None


@dataclass
class MediaItemAttributes:
    persistent_id: Annotated[Optional[Uint64], 0] = None
    title: Annotated[Optional[str], 1] = None
    duration_ms: Annotated[Optional[Uint32], 4] = None
    album: Annotated[Optional[str], 6] = None
    artist: Annotated[Optional[str], 12] = None
    album_artist: Annotated[Optional[str], 14] = None
    genre: Annotated[Optional[str], 16] = None
    artwork_ftid: Annotated[Optional[Uint8], 26] = None


@dataclass
class PlaybackAttributes:
    status: Annotated[Optional[PlaybackStatus], 0] = None
    elapsed_ms: Annotated[Optional[Uint32], 1] = None
    app_name: Annotated[Optional[str], 7] = None
    app_bundle_id: Annotated[Optional[str], 16] = None


@csm(0x5001)
@dataclass
class NowPlayingUpdate:
    media_item_attributes: Annotated[Optional[MediaItemAttributes], 0] = None
    playback_attributes: Annotated[Optional[PlaybackAttributes], 1] = None


@csm(0x5002)
@dataclass
class StopNowPlayingUpdates:
    pass
