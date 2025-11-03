/* eslint-disable no-restricted-globals */
import { decodeTypeMap } from "../../../../main/carplay/messages";
import { AudioPlayerKey } from "./types";
import { RingBuffer } from "ringbuf.js";
import { createAudioPlayerKey } from "./utils";

/**
 * CarPlay.worker
 * - Receives steady ~60 ms PCM chunks (5760 samples @ 48 kHz stereo).
 * - Normalizes to Int16, preserves channel-frame alignment.
 * - Accumulates across packets so that only multiples of 128 frames are queued.
 *   -> Target slice = 384 frames (~8 ms @ 48 kHz).
 * - Pacer ticks ~8 ms and pushes exactly one full slice per tick (opt. catch-up).
 */

type Key = AudioPlayerKey;

const WORKLET_QUANTUM = 128; // frames
const PCM_TIMEOUT = 2000;    // new-stream detection

// SAB ring for each audio stream
const audioBuffers: Record<Key, RingBuffer> = {};
const sliceQueues: Record<Key, Int16Array[]> = {};
const accumSamples: Record<Key, Int16Array | undefined> = {};
const pendingSlices: Record<Key, Int16Array[]> = {};

// Steady feeder
type Pacer = {
  id: number | null;
  msPerTick: number;
  framesPerTick: number;
  sampleRate: number;
  channels: number;
};
const pacers: Record<Key, Pacer> = {};

let microphonePort: MessagePort | undefined;
let isNewStream = true;
let lastPcmTimestamp = Date.now();

function framesPerSliceAligned(sampleRate: number): number {
  const targetFrames = Math.max(WORKLET_QUANTUM, Math.round(sampleRate * 0.008));
  const multiples = Math.max(1, Math.round(targetFrames / WORKLET_QUANTUM));
  return multiples * WORKLET_QUANTUM;
}

function ensurePacer(key: Key, sampleRate: number, channels: number) {
  const frames = framesPerSliceAligned(sampleRate);
  const ms = (frames / sampleRate) * 1000;

  const existing = pacers[key];
  if (existing && existing.sampleRate === sampleRate && existing.channels === channels) return;

  if (existing?.id != null) clearInterval(existing.id);

  const pacer: Pacer = {
    id: null,
    msPerTick: ms,
    framesPerTick: frames,
    sampleRate,
    channels,
  };

  const tick = () => {
    const q = sliceQueues[key];
    const rb = audioBuffers[key];
    if (!rb || !q || q.length === 0) return;

    // Push exactly one full slice per tick for steady cadence
    const slice = q.shift()!;
    rb.push(slice);

    // If queue grows large, occasionally push a second slice
    if (q.length > 40) {
      const extra = q.shift();
      if (extra) rb.push(extra);
    }
  };

  const timer = setInterval(tick, pacer.msPerTick) as unknown as number;
  pacer.id = timer;
  pacers[key] = pacer;
}

function extractFullSlices(
  key: Key,
  incoming: Int16Array,
  sampleRate: number,
  channels: number
): Int16Array[] {
  const framesPerSlice = framesPerSliceAligned(sampleRate);
  const sliceSamples = framesPerSlice * channels;

  // Merge with previous accumulator
  const prev = accumSamples[key];
  let src = incoming;
  if (prev && prev.length) {
    const merged = new Int16Array(prev.length + incoming.length);
    merged.set(prev, 0);
    merged.set(incoming, prev.length);
    src = merged;
  }

  // Use only whole frames
  const totalFrames = Math.floor(src.length / channels);
  const usableSamples = totalFrames * channels;

  // Number of full slices
  const fullSlices = Math.floor((usableSamples) / sliceSamples);
  const out: Int16Array[] = [];

  if (fullSlices > 0) {
    for (let i = 0; i < fullSlices; i++) {
      const start = i * sliceSamples;
      const end = start + sliceSamples;
      out.push(src.subarray(start, end));
    }
  }

  // Keep leftover
  const consumedSamples = fullSlices * sliceSamples;
  const leftoverSamples = usableSamples - consumedSamples;
  if (leftoverSamples > 0) {
    accumSamples[key] = src.subarray(consumedSamples, consumedSamples + leftoverSamples);
  } else {
    accumSamples[key] = undefined;
  }
  return out;
}

function processAudioData(audioData: any) {
  const { decodeType, audioType } = audioData;
  const meta = decodeTypeMap[decodeType];

  // Normalize to Int16Array
  let int16: Int16Array;
  if (audioData.data instanceof Int16Array) {
    int16 =
      audioData.data.byteOffset % 2 === 0 &&
      audioData.data.buffer.byteLength >= audioData.data.byteOffset + audioData.data.byteLength
        ? audioData.data
        : new Int16Array(audioData.data);
  } else if (audioData.buffer instanceof ArrayBuffer) {
    int16 = new Int16Array(audioData.buffer);
  } else if (audioData.chunk instanceof ArrayBuffer) {
    int16 = new Int16Array(audioData.chunk);
  } else {
    // eslint-disable-next-line no-console
    console.error("[CARPLAY.WORKER] PCM - cannot interpret PCM data:", audioData);
    return;
  }

  const now = Date.now();
  if (now - lastPcmTimestamp > PCM_TIMEOUT) isNewStream = true;

  if (isNewStream && meta) {
    isNewStream = false;
    (self as unknown as Worker).postMessage({
      type: "audioInfo",
      payload: {
        codec: meta.format ?? meta.mimeType ?? String(decodeType),
        sampleRate: meta.frequency,
        channels: meta.channel,
        bitDepth: meta.bitDepth,
      },
    });
    const keyInit = createAudioPlayerKey(decodeType, audioType);
    accumSamples[keyInit] = undefined;
  }

  // Downmix for UI/FFT
  if (meta) {
    const chUI = Math.max(1, meta.channel ?? 2);
    const framesUI = Math.floor(int16.length / chUI);
    const float32 = new Float32Array(framesUI);
    for (let i = 0; i < framesUI; i++) {
      let sum = 0;
      for (let c = 0; c < chUI; c++) sum += int16[i * chUI + c] || 0;
      float32[i] = (sum / chUI) / 32768;
    }
    (self as unknown as Worker).postMessage(
      { type: "pcmData", payload: float32.buffer, decodeType },
      [float32.buffer]
    );
  }

  const key = createAudioPlayerKey(decodeType, audioType);
  const channels = Math.max(1, meta?.channel ?? 2);
  const sr = Math.max(8000, meta?.frequency ?? 48000);

  // Accumulate and extract full 128*n slices
  const slices = extractFullSlices(key, int16, sr, channels);

  if (slices.length > 0) {
    if (audioBuffers[key]) {
      if (!sliceQueues[key]) sliceQueues[key] = [];
      for (const s of slices) sliceQueues[key].push(s);
      ensurePacer(key, sr, channels);
    } else {
      // Buffer until the SAB ring arrives
      pendingSlices[key] = pendingSlices[key] || [];
      for (const s of slices) pendingSlices[key].push(s);
      (self as unknown as Worker).postMessage({
        type: "requestBuffer",
        message: { decodeType, audioType },
      });
    }
  }

  lastPcmTimestamp = now;
}

function setupPorts(mPort: MessagePort) {
  try {
    mPort.onmessage = (ev) => {
      try {
        const data = ev.data as any;
        if (data.type === "audio" && (data.buffer || data.data || data.chunk)) {
          processAudioData(data);
        }
      } catch (e) {
        // eslint-disable-next-line no-console
        console.error("[CARPLAY.WORKER] error processing audio message:", e);
      }
    };
    mPort.start?.();
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[CARPLAY.WORKER] port setup failed:", e);
    (self as unknown as Worker).postMessage({ type: "failure", error: "Port setup failed" });
  }
}

(self as unknown as Worker).onmessage = (ev: MessageEvent) => {
  const data = ev.data as any;
  switch (data.type) {
    case "initialise": {
      microphonePort = data.payload.microphonePort;
      if (microphonePort) setupPorts(microphonePort);
      else console.error("[CARPLAY.WORKER] missing microphonePort in initialise payload");
      break;
    }
    case "audioPlayer": {
      const { sab, decodeType, audioType } = data.payload as {
        sab: SharedArrayBuffer;
        decodeType: number;
        audioType: number;
      };
      const key = createAudioPlayerKey(decodeType, audioType);
      audioBuffers[key] = new RingBuffer(sab, Int16Array);

      // Drain any pending full slices via the pacer
      const pend = pendingSlices[key] || [];
      if (pend.length) {
        if (!sliceQueues[key]) sliceQueues[key] = [];
        for (const s of pend) sliceQueues[key].push(s);
        const meta = decodeTypeMap[decodeType];
        const sr = Math.max(8000, meta?.frequency ?? 48000);
        const ch = Math.max(1, meta?.channel ?? 2);
        ensurePacer(key, sr, ch);
        delete pendingSlices[key];
      }
      break;
    }
    case "stop": {
      isNewStream = true;
      Object.keys(pacers).forEach((k) => {
        const p = pacers[k as Key];
        if (p?.id != null) clearInterval(p.id);
        delete pacers[k as Key];
      });
      break;
    }
    default:
      break;
  }
};

export {};
