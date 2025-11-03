import { useCallback, useEffect, useRef } from 'react'
import { AudioCommand, AudioData, decodeTypeMap } from '../../../main/carplay/messages'
import { PcmPlayer } from '../audio/PcmPlayer'
import { AudioPlayerKey, CarPlayWorker } from './worker/types'
import { createAudioPlayerKey } from './worker/utils'
import { useCarplayStore } from '../store/store'

type PlayerEntry = {
  player: PcmPlayer
  isNav: boolean
  jitterMs: number
}

const useCarplayAudio = (worker: CarPlayWorker) => {
  const playersRef = useRef(new Map<AudioPlayerKey, PlayerEntry>())

  const audioVolume = useCarplayStore((s) => s.audioVolume ?? s.settings?.audioVolume ?? 1.0)
  const navVolume = useCarplayStore((s) => s.navVolume ?? s.settings?.navVolume ?? 0.5)
  const audioJitterMs = useCarplayStore((s) => s.audioJitterMs ?? 15)

  const getCommandName = (cmd?: number) => {
    if (typeof cmd === 'number') {
      // Numeric enum reverse mapping
      return (AudioCommand as any)[cmd] as string | undefined
    }
    return undefined
  }

  const applyVolumes = useCallback(() => {
    playersRef.current.forEach(({ player, isNav }) => {
      player.volume(isNav ? navVolume : audioVolume)
    })
  }, [audioVolume, navVolume])

  useEffect(() => {
    applyVolumes()
  }, [applyVolumes])

  const getAudioPlayer = useCallback(
    (audio: AudioData): PcmPlayer => {
      const { decodeType, audioType } = audio
      const format = decodeTypeMap[decodeType]
      const key: AudioPlayerKey = createAudioPlayerKey(decodeType, audioType)
      const isNav = audioType === 2 || audioType === 3

      const existing = playersRef.current.get(key)

      if (!existing || existing.jitterMs !== audioJitterMs) {
        if (existing) {
          try {
            existing.player.stop()
          } catch {}
          playersRef.current.delete(key)
        }

        // eslint-disable-next-line no-console
        console.log(
          '[Audio] Create PcmPlayer FS:',
          format.frequency,
          'Hz',
          'Channels:',
          format.channel,
          'Jitter:',
          audioJitterMs,
          'ms'
        )

        const player = new PcmPlayer(format.frequency, format.channel, audioJitterMs)
        playersRef.current.set(key, { player, isNav, jitterMs: audioJitterMs })
        player.start()

        // Initial volume based on stream type
        player.volume(isNav ? navVolume : audioVolume)

        // Hand off SAB to the worker
        worker.postMessage({
          type: 'audioPlayer',
          payload: {
            sab: player.getRawBuffer(),
            decodeType,
            audioType
          }
        })

        return player
      }

      return existing.player
    },
    [audioJitterMs, audioVolume, navVolume, worker]
  )

  const processAudio = useCallback(
    (audio: AudioData) => {
      const player = getAudioPlayer(audio)
      // eslint-disable-next-line no-console
      console.log(
        '[Audio]',
        'decodeType:',
        audio.decodeType,
        'audioType:',
        audio.audioType,
        'command:',
        audio.command,
        `(${getCommandName(audio.command)})`
      )

      if (audio.command === AudioCommand.AudioNaviStart) {
        setTimeout(() => player.volume(navVolume), 10)
      } else if (audio.volumeDuration && typeof audio.volume === 'number') {
        player.volume(audio.volume, audio.volumeDuration)
      }
    },
    [getAudioPlayer, navVolume]
  )

  useEffect(() => {
    return () => {
      playersRef.current.forEach((entry) => {
        try {
          entry.player.stop()
        } catch {}
      })
      playersRef.current.clear()
    }
  }, [])

  return { processAudio, getAudioPlayer }
}

export default useCarplayAudio
