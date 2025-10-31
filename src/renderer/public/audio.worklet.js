"use strict";

// AudioWorklet global declarations
declare const sampleRate: number;
declare function registerProcessor(name: string, ctor: any): void;
declare class AudioWorkletProcessor {
  readonly port: MessagePort;
  constructor(options?: any);
}

// Constants
const RENDER_QUANTUM_FRAMES = 128;
const RING_POINTERS_SIZE = 8;
const START_QUANTA = 3;

// Ring buffer reader over SharedArrayBuffer (Int16 interleaved)
class RingBuffReader {
  private storage: Int16Array;
  private writePointer: Uint32Array;
  private readPointer: Uint32Array;

  constructor(buffer: SharedArrayBuffer) {
    const storageSize =
      (buffer.byteLength - RING_POINTERS_SIZE) / Int16Array.BYTES_PER_ELEMENT;
    this.storage = new Int16Array(buffer, RING_POINTERS_SIZE, storageSize);
    this.writePointer = new Uint32Array(buffer, 0, 1);
    this.readPointer = new Uint32Array(buffer, 4, 1);
  }

  readTo(target: Int16Array): number {
    const { readPos, available } = this.getReadInfo();
    if (available === 0) return 0;

    const readLength = Math.min(available, target.length);
    const first = Math.min(this.storage.length - readPos, readLength);
    const second = readLength - first;

    target.set(this.storage.subarray(readPos, readPos + first), 0);
    if (second > 0) target.set(this.storage.subarray(0, second), first);

    Atomics.store(
      this.readPointer,
      0,
      (readPos + readLength) % this.storage.length
    );
    return readLength;
  }

  getReadInfo() {
    const readPos = Atomics.load(this.readPointer, 0);
    const writePos = Atomics.load(this.writePointer, 0);
    const available =
      (writePos + this.storage.length - readPos) % this.storage.length;
    return { readPos, writePos, available };
  }
}

// PCM Worklet
class PCMWorkletProcessor extends AudioWorkletProcessor {
  private channels: number;
  private reader: RingBuffReader;
  private readerOutput: Int16Array;

  private primed = false;

  // last-sample hold
  private _lastL = 0;
  private _lastR = 0;
  private _lastM = 0;

  // soft start / recovery ramp
  private _needRamp = true;   // ramp at first audible output and after underruns
  private _rampLen = 0;       // total ramp length in samples
  private _rampLeft = 0;      // remaining samples to ramp
  private _xfFromL = 0;       // crossfade (L)
  private _xfFromR = 0;       // crossfade (R)
  private _xfFromM = 0;       // crossfade mono)
  private _rampSR = sampleRate;

  constructor(options: any) {
    super();
    const { sab, channels, streamSampleRate } = options.processorOptions as {
      sab: SharedArrayBuffer;
      channels: number;
      streamSampleRate?: number;
    };

    this.channels = Math.max(1, channels | 0);
    this.reader = new RingBuffReader(sab);
    this.readerOutput = new Int16Array(RENDER_QUANTUM_FRAMES * this.channels);

    if (typeof streamSampleRate === "number" && streamSampleRate > 0) {
      this._rampSR = streamSampleRate;
    }
  }

  private toFloat32(s16: number) {
    return s16 / 32768;
  }

  private beginRamp() {
    const RAMP_MS = 8;
    this._rampLen = Math.max(1, Math.floor((this._rampSR * RAMP_MS) / 1000));
    this._rampLeft = this._rampLen;

    // capture previous (held) values to crossfade from
    this._xfFromL = this._lastL;
    this._xfFromR = this._lastR;
    this._xfFromM = this._lastM;

    this._needRamp = false;
  }

  process(_inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const out = outputs[0];
    const frames = RENDER_QUANTUM_FRAMES;
    const ch = this.channels;
    const needSamples = frames * ch;

    // preroll
    const { available } = this.reader.getReadInfo();
    if (!this.primed) {
      if (available >= START_QUANTA * needSamples) {
        this.primed = true;
        this._needRamp = true; // ramp on first audible block
      } else {
        for (let c = 0; c < out.length; c++) out[c].fill(0);
        return true;
      }
    }

    // read one quantum
    const got = this.reader.readTo(this.readerOutput);
    const framesGot = Math.floor(got / ch);

    // if nothing available, hold last sample and request a ramp on recovery
    if (framesGot === 0) {
      if (out.length >= 2) {
        const L = out[0], R = out[1] ?? out[0];
        for (let f = 0; f < frames; f++) {
          L[f] = this._lastL;
          R[f] = this._lastR;
        }
      } else {
        const M = out[0];
        for (let f = 0; f < frames; f++) M[f] = this._lastM;
      }
      this._needRamp = true;
      return true;
    }

    // ramp will start on first block with data after start/underrun
    if (this._needRamp && this._rampLeft === 0) this.beginRamp();

    if (ch === 2) {
      const L = out[0];
      const R = out[1];

      // write deinterleaved samples
      let f = 0, i = 0;
      for (; f < framesGot; f++, i += 2) {
        L[f] = this.toFloat32(this.readerOutput[i]);
        R[f] = this.toFloat32(this.readerOutput[i + 1]);
      }

      // apply ramp (crossfade from held last values) on the first samples
      if (this._rampLeft > 0) {
        const rampIndexStart = this._rampLen - this._rampLeft;
        const doN = Math.min(f, this._rampLeft);
        for (let k = 0; k < doN; k++) {
          const idx = k; // within current block
          const a = (rampIndexStart + k + 1) / this._rampLen; // 0→1
          const b = 1 - a;
          L[idx] = b * this._xfFromL + a * L[idx];
          R[idx] = b * this._xfFromR + a * R[idx];
        }
        this._rampLeft -= doN;
        if (this._rampLeft <= 0) this._rampLeft = 0;
      }

      // pad rest of block if we didn't get a full quantum
      const padL = f ? L[f - 1] : this._lastL;
      const padR = f ? R[f - 1] : this._lastR;
      for (; f < frames; f++) {
        L[f] = padL;
        R[f] = padR;
      }

      // remember last outputs (for hold/pad and next ramp origin)
      this._lastL = L[frames - 1];
      this._lastR = R[frames - 1];

      // if we had to pad, request ramp on recovery
      if (framesGot < frames) this._needRamp = true;
    } else {
      // mono (or treat any non-2 channel stream as mono into first channel)
      const M = out[0];

      // write mono samples
      let f = 0;
      for (; f < framesGot; f++) {
        M[f] = this.toFloat32(this.readerOutput[f]);
      }

      // apply ramp on the first samples
      if (this._rampLeft > 0) {
        const rampIndexStart = this._rampLen - this._rampLeft;
        const doN = Math.min(f, this._rampLeft);
        for (let k = 0; k < doN; k++) {
          const idx = k;
          const a = (rampIndexStart + k + 1) / this._rampLen;
          const b = 1 - a;
          M[idx] = b * this._xfFromM + a * M[idx];
        }
        this._rampLeft -= doN;
        if (this._rampLeft <= 0) this._rampLeft = 0;
      }

      // pad remaining frames
      const pad = f ? M[f - 1] : this._lastM;
      for (; f < frames; f++) M[f] = pad;

      // clear any extra output channels
      for (let c = 1; c < out.length; c++) out[c].fill(0);

      // remember last outputs
      this._lastM = M[frames - 1];

      if (framesGot < frames) this._needRamp = true;
    }

    return true;
  }
}

registerProcessor("pcm-worklet-processor", PCMWorkletProcessor);
