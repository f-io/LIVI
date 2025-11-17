type PortAudioLike = {
  type?: 'audio'
  buffer?: ArrayBuffer
  data?: Int16Array
  chunk?: ArrayBuffer
}

type AudioDataMsg = PortAudioLike & {
  decodeType?: number
  audioType?: number
}

function toInt16(msg: unknown): Int16Array | undefined {
  if (typeof msg === 'object' && msg !== null) {
    const a = msg as PortAudioLike
    if (a.data instanceof Int16Array) {
      const src = a.data
      const aligned =
        src.byteOffset % 2 === 0 && src.buffer.byteLength >= src.byteOffset + src.byteLength
      return aligned ? src : new Int16Array(src)
    }
    if (a.buffer instanceof ArrayBuffer) return new Int16Array(a.buffer)
    if (a.chunk instanceof ArrayBuffer) return new Int16Array(a.chunk)
  }
  console.error('[CARPLAY.WORKER] PCM - cannot interpret PCM data:', msg)
  return undefined
}

function processAudioData(audioData: AudioDataMsg) {
  const decodeType = audioData.decodeType
  const pcm = toInt16(audioData)
  if (!pcm) return

  const frames = pcm.length
  const f32 = new Float32Array(frames)

  for (let i = 0; i < frames; i++) {
    f32[i] = (pcm[i] || 0) / 32768
  }

  ;(self as unknown as Worker).postMessage(
    {
      type: 'pcmData',
      payload: f32.buffer,
      decodeType
    },
    [f32.buffer]
  )
}

function setupPorts(port: MessagePort) {
  try {
    port.onmessage = (ev: MessageEvent<AudioDataMsg>) => {
      try {
        const data = ev.data
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

type InitialiseCommand = {
  type: 'initialise'
  payload?: {
    audioPort?: MessagePort
  }
}

type StopCommand = {
  type: 'stop'
}

type Command = InitialiseCommand | StopCommand | { type: string; payload?: unknown }
;(self as unknown as Worker).onmessage = (ev: MessageEvent<Command>) => {
  const data = ev.data
  switch (data?.type) {
    case 'initialise': {
      const payload = (data as InitialiseCommand).payload
      const port = payload?.audioPort
      if (port) {
        setupPorts(port)
      } else {
        console.error('[CARPLAY.WORKER] missing audioPort in initialise payload')
      }
      break
    }
    case 'stop': {
      break
    }
    default:
      break
  }
}

export {}
