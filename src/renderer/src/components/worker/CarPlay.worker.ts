/* eslint-disable no-restricted-globals */
import { decodeTypeMap } from '../../../../main/carplay/messages'
import { AudioPlayerKey } from './types'
import { RingBuffer } from 'ringbuf.js'
import { createAudioPlayerKey } from './utils'

type Key = AudioPlayerKey

const audioBuffers: Record<Key, RingBuffer> = {}
const pendingChunks: Record<Key, Int16Array[]> = {}
const sabRequested: Record<Key, boolean> = {}

let audioPort: MessagePort | undefined

type Info = { codec: string | number; sampleRate: number; channels: number; bitDepth?: number }
const lastInfo: Record<Key, Info> = {}
let currentKey: Key | undefined

function toInt16(audioData: any): Int16Array | undefined {
  if (audioData?.data instanceof Int16Array) {
    const src = audioData.data as Int16Array
    const aligned =
      src.byteOffset % 2 === 0 && src.buffer.byteLength >= src.byteOffset + src.byteLength
    return aligned ? src : new Int16Array(src)
  }
  if (audioData?.buffer instanceof ArrayBuffer) return new Int16Array(audioData.buffer)
  if (audioData?.chunk instanceof ArrayBuffer) return new Int16Array(audioData.chunk)
  console.error('[CARPLAY.WORKER] PCM - cannot interpret PCM data:', audioData)
  return undefined
}

function requestSabIfNeeded(decodeType: number, audioType: number, key: Key) {
  if (!audioBuffers[key] && !sabRequested[key]) {
    ;(self as unknown as Worker).postMessage({
      type: 'requestBuffer',
      message: { decodeType, audioType }
    })
    sabRequested[key] = true
  }
}

function pushOrPend(key: Key, chunk: Int16Array) {
  const rb = audioBuffers[key]
  if (rb) rb.push(chunk)
  else {
    if (!pendingChunks[key]) pendingChunks[key] = []
    pendingChunks[key].push(chunk)
  }
}

function processAudioData(audioData: any) {
  const { decodeType, audioType } = audioData
  const key = createAudioPlayerKey(decodeType, audioType)
  const meta = decodeTypeMap[decodeType]

  const channels = Math.max(1, meta?.channel ?? 2)
  const sampleRate = Math.max(8000, meta?.frequency ?? 48000)
  const codec = meta?.format ?? meta?.mimeType ?? String(decodeType)
  const bitDepth = meta?.bitDepth

  const pcm = toInt16(audioData)
  if (!pcm) return

  requestSabIfNeeded(decodeType, audioType, key)

  // send audioInfo on key change or if format values differ
  const info: Info = { codec, sampleRate, channels, bitDepth }
  const keyChanged = key !== currentKey
  const changed =
    !lastInfo[key] ||
    lastInfo[key].sampleRate !== info.sampleRate ||
    lastInfo[key].channels !== info.channels ||
    lastInfo[key].codec !== info.codec ||
    lastInfo[key].bitDepth !== info.bitDepth

  if (keyChanged || changed) {
    currentKey = key
    lastInfo[key] = info
    ;(self as unknown as Worker).postMessage({ type: 'audioInfo', payload: info })
  }

  // FFT downmix for UI/visuals
  {
    const frames = Math.floor(pcm.length / channels)
    const f32 = new Float32Array(frames)
    for (let i = 0; i < frames; i++) {
      let s = 0
      for (let c = 0; c < channels; c++) s += pcm[i * channels + c] || 0
      f32[i] = s / channels / 32768
    }
    ;(self as unknown as Worker).postMessage({ type: 'pcmData', payload: f32.buffer, decodeType }, [
      f32.buffer
    ])
  }

  pushOrPend(key, pcm)
}

function setupPorts(port: MessagePort) {
  try {
    port.onmessage = (ev) => {
      try {
        const data = ev.data as any
        if (data?.type === 'audio' && (data.buffer || data.data || data.chunk)) {
          processAudioData(data)
        }
      } catch (e) {
        console.error('[CARPLAY.WORKER] error processing audio message:', e)
      }
    }
    port.start?.()
  } catch (e) {
    console.error('[CARPLAY.WORKER] port setup failed:', e)
    ;(self as unknown as Worker).postMessage({ type: 'failure', error: 'Port setup failed' })
  }
}

;(self as unknown as Worker).onmessage = (ev: MessageEvent) => {
  const data = ev.data as any
  switch (data?.type) {
    case 'initialise': {
      audioPort = data?.payload?.audioPort
      if (audioPort) setupPorts(audioPort)
      else console.error('[CARPLAY.WORKER] missing audioPort in initialise payload')
      break
    }
    case 'audioPlayer': {
      const { sab, decodeType, audioType } = data.payload as {
        sab: SharedArrayBuffer
        decodeType: number
        audioType: number
      }
      const key = createAudioPlayerKey(decodeType, audioType)
      audioBuffers[key] = new RingBuffer(sab, Int16Array)
      sabRequested[key] = false

      const pend = pendingChunks[key] || []
      if (pend.length) {
        for (const chunk of pend) audioBuffers[key].push(chunk)
        delete pendingChunks[key]
      }
      break
    }
    case 'stop': {
      Object.keys(audioBuffers).forEach((k) => delete audioBuffers[k as Key])
      Object.keys(pendingChunks).forEach((k) => delete pendingChunks[k as Key])
      Object.keys(sabRequested).forEach((k) => delete sabRequested[k as Key])
      Object.keys(lastInfo).forEach((k) => delete lastInfo[k as Key])
      currentKey = undefined
      break
    }
    default:
      break
  }
}

export {}
