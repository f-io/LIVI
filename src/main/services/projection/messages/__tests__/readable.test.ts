import {
  AudioData,
  BoxPhase,
  BoxUpdateState,
  BoxUpdateStatus,
  boxPhaseToString,
  boxUpdateStatusToString,
  DongleReady,
  DuckAudio,
  MediaData,
  Message,
  Unplugged
} from '@main/services/projection/messages'

describe('readable messages', () => {
  test('DongleReady and Unplugged construct as messages', () => {
    const ready = new DongleReady()
    const unplugged = new Unplugged()

    expect(ready).toBeInstanceOf(Message)
    expect(unplugged).toBeInstanceOf(Message)
  })

  test('MediaData keeps unknown media type payload undefined', () => {
    const msg = new MediaData(999 as any)

    expect(msg.mediaType).toBe(999)
    expect(msg.payload).toBeUndefined()
  })

  test('DuckAudio clamps level to [0,1] and duration to >= 0', () => {
    const msg = new DuckAudio(0.5, 200)
    expect(msg.level).toBe(0.5)
    expect(msg.durationMs).toBe(200)

    const clamped = new DuckAudio(2, -50)
    expect(clamped.level).toBe(1)
    expect(clamped.durationMs).toBe(0)

    expect(new DuckAudio(-1, 0).level).toBe(0)
  })

  test('boxPhaseToString returns enum name and fallback for unknown values', () => {
    expect(boxPhaseToString(BoxPhase.EVT_BOX_READY)).toBe('EVT_BOX_READY')
    expect(boxPhaseToString(9999)).toBe('UNKNOWN_PHASE_9999')
  })

  test('boxUpdateStatusToString maps known statuses and unknown fallback', () => {
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxUpdateStart)).toBe('EVT_BOX_UPDATE')
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxUpdateSuccess)).toBe('EVT_BOX_UPDATE_SUCCESS')
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxUpdateFailed)).toBe('EVT_BOX_UPDATE_FAILED')
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxOtaUpdateStart)).toBe('EVT_BOX_OTA_UPDATE')
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxOtaUpdateSuccess)).toBe(
      'EVT_BOX_OTA_UPDATE_SUCCESS'
    )
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxOtaUpdateFailed)).toBe(
      'EVT_BOX_OTA_UPDATE_FAILED'
    )
    expect(boxUpdateStatusToString(999)).toBe('EVT_BOX_UPDATE_UNKNOWN(999)')
  })

  test('BoxUpdateState maps success terminal state', () => {
    const msg = new BoxUpdateState(BoxUpdateStatus.BoxUpdateSuccess)

    expect(msg.status).toBe(BoxUpdateStatus.BoxUpdateSuccess)
    expect(msg.statusText).toBe('EVT_BOX_UPDATE_SUCCESS')
    expect(msg.isOta).toBe(false)
    expect(msg.isTerminal).toBe(true)
    expect(msg.ok).toBe(true)
  })

  test('BoxUpdateState maps failed ota terminal state', () => {
    const msg = new BoxUpdateState(BoxUpdateStatus.BoxOtaUpdateFailed)

    expect(msg.status).toBe(BoxUpdateStatus.BoxOtaUpdateFailed)
    expect(msg.statusText).toBe('EVT_BOX_OTA_UPDATE_FAILED')
    expect(msg.isOta).toBe(true)
    expect(msg.isTerminal).toBe(true)
    expect(msg.ok).toBe(false)
  })

  test('BoxUpdateState maps non-terminal start state', () => {
    const msg = new BoxUpdateState(BoxUpdateStatus.BoxUpdateStart)

    expect(msg.isTerminal).toBe(false)
    expect(msg.ok).toBeUndefined()
  })
})
