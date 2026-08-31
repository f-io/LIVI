#ifndef CP_VIDEO_NAL_H
#define CP_VIDEO_NAL_H

#include <stddef.h>
#include <stdint.h>

// Implemented by the livi-video-nal crate (rust/nal); the enum order is part of the ABI.

#ifdef __cplusplus
extern "C" {
#endif

typedef enum { CP_CODEC_H264, CP_CODEC_H265 } CpCodec;

typedef enum { CP_NAL_DELTA = 0, CP_NAL_PARAMS = 1, CP_NAL_KEYFRAME = 2 } CpNalKind;

CpCodec cp_detect_codec(const uint8_t* payload, size_t payload_len,
                        const uint8_t** codec_data, size_t* codec_data_len);

CpNalKind cp_classify_nal(const uint8_t* frame, size_t frame_len, CpCodec codec);

#ifdef __cplusplus
}
#endif

#endif
