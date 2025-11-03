import { useCallback, useEffect, useState } from 'react'
import { AudioCommand, AudioData, decodeTypeMap } from '@main/carplay/messages'
import { PcmPlayer } from '../../../../audio/PcmPlayer'
import { useCarplayStore } from '../../../../store/store'
import { createAudioPlayerKey } from '@worker/utils'
import { AudioPlayerKey, CarPlayWorker } from '@worker/types'

const useCarplayAudio = (worker: CarPlayWorker) => {
  const [audioPlayers] = useState(new Map<AudioPlayerKey, PcmPlayer>())
  const audioVolume = useCarplayStore(s => s.settings?.audioVolume ?? 1.0)
  const navVolume = useCarplayStore(s => s.settings?.navVolume ?? 0.5)

  useEffect(() => {
    audioPlayers.forEach((player, key) => {
      if (key.includes('navi') || key.endsWith('2') || key.endsWith('3')) {
        player.volume(navVolume)
      } else {
        player.volume(audioVolume)
      }
    })
  }, [audioVolume, navVolume, audioPlayers])

  const getCommandName = (cmd?: number) => {
    if (typeof cmd === 'number' && cmd in AudioCommand) {
      return AudioCommand[cmd as unknown as keyof typeof AudioCommand]
    }
    return undefined
  }

  const getAudioPlayer = useCallback(
    (audio: AudioData): PcmPlayer => {
      const { decodeType, audioType } = audio
      const format = decodeTypeMap[decodeType]
      const audioKey = createAudioPlayerKey(decodeType, audioType)

      let player = audioPlayers.get(audioKey)
      if (!player) {
        player = new PcmPlayer(format.frequency, format.channel)
        audioPlayers.set(audioKey, player)
        player.start()
        worker.postMessage({
          type: 'audioPlayer',
          payload: {
            sab: player.getRawBuffer(),
            decodeType,
            audioType,
          },
        })
      }

      const isNav = audioType === 2 || audioType === 3
      player.volume(isNav ? navVolume : audioVolume)

      return player
    },
    [audioPlayers, worker, audioVolume, navVolume]
  )

  const processAudio = useCallback(
    (audio: AudioData) => {
      const player = getAudioPlayer(audio)
      console.log('[Audio] decodeType:', audio.decodeType, 'audioType:', audio.audioType, 'command:', audio.command, '(', getCommandName(audio.command), ')')

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
      audioPlayers.forEach(p => p.stop())
    }
  }, [audioPlayers])

  return { processAudio, getAudioPlayer }
}

export default useCarplayAudio
