export type AudioFormat = {
  frequency: 48000 | 44100 | 24000 | 16000 | 8000
  channel: 1 | 2
  bitDepth: number
  format?: string
  mimeType?: string
}

type DecodeTypeMapping = {
  [key: number]: AudioFormat
}

export const decodeTypeMap: DecodeTypeMapping = {
  1: {
    frequency: 44100,
    channel: 2,
    bitDepth: 16,
    format: 's16le',
    mimeType: 'audio/L16; rate=44100; channels=2'
  },
  2: {
    frequency: 44100,
    channel: 2,
    bitDepth: 16,
    format: 's16le',
    mimeType: 'audio/L16; rate=44100; channels=2'
  },
  3: {
    frequency: 8000,
    channel: 1,
    bitDepth: 16,
    format: 's16le',
    mimeType: 'audio/L16; rate=8000; channels=1'
  },
  4: {
    frequency: 48000,
    channel: 2,
    bitDepth: 16,
    format: 's16le',
    mimeType: 'audio/L16; rate=48000; channels=2'
  },
  5: {
    frequency: 16000,
    channel: 1,
    bitDepth: 16,
    format: 's16le',
    mimeType: 'audio/L16; rate=16000; channels=1'
  },
  6: {
    frequency: 24000,
    channel: 1,
    bitDepth: 16,
    format: 's16le',
    mimeType: 'audio/L16; rate=24000; channels=1'
  },
  7: {
    frequency: 16000,
    channel: 2,
    bitDepth: 16,
    format: 's16le',
    mimeType: 'audio/L16; rate=16000; channels=2'
  }
}
