use crate::{csm_enum, csm_group, csm_message};

csm_enum! {
    pub enum PlaybackStatus {
        Stopped = 0,
        Playing = 1,
        Paused = 2,
        SeekForward = 3,
        SeekBackward = 4,
    }
}

csm_group! {
    pub struct StartMediaItemAttributes {
        0 => persistent_id: [flag],
        1 => title: [flag],
        4 => duration_ms: [flag],
        6 => album: [flag],
        12 => artist: [flag],
        14 => album_artist: [flag],
        16 => genre: [flag],
        26 => artwork: [flag],
    }
}

csm_group! {
    pub struct StartPlaybackAttributes {
        0 => status: [flag],
        1 => elapsed_ms: [flag],
        7 => app_name: [flag],
        16 => app_bundle_id: [flag],
    }
}

csm_message! {
    pub struct StartNowPlayingUpdates = 0x5000 {
        0 => media_item_attributes: [opt group StartMediaItemAttributes],
        1 => playback_attributes: [opt group StartPlaybackAttributes],
    }
}

csm_group! {
    pub struct MediaItemAttributes {
        0 => persistent_id: [opt u64],
        1 => title: [opt str],
        4 => duration_ms: [opt u32],
        6 => album: [opt str],
        12 => artist: [opt str],
        14 => album_artist: [opt str],
        16 => genre: [opt str],
        26 => artwork_ftid: [opt u8],
    }
}

csm_group! {
    pub struct PlaybackAttributes {
        0 => status: [opt enum PlaybackStatus],
        1 => elapsed_ms: [opt u32],
        7 => app_name: [opt str],
        16 => app_bundle_id: [opt str],
    }
}

csm_message! {
    pub struct NowPlayingUpdate = 0x5001 {
        0 => media_item_attributes: [opt group MediaItemAttributes],
        1 => playback_attributes: [opt group PlaybackAttributes],
    }
}

csm_message! {
    pub struct StopNowPlayingUpdates = 0x5002 {}
}
