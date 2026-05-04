import { Microphone } from '@main/services/audio'
import type { DongleConfig } from '@shared/types'
import { AudioCommand, type CommandValue } from '@shared/types/ProjectionEnums'
import { webusb } from 'usb'
import { DEFAULT_CONFIG, DongleDriver } from '../driver/dongle/dongleDriver'
import {
  AudioData,
  Command,
  MediaData,
  Message,
  Plugged,
  SendAudio,
  SendCommand,
  SendTouch,
  Unplugged,
  VideoData
} from '../messages'

export type ProjectionMessage =
  | { type: 'plugged'; message?: undefined }
  | { type: 'dongleInfo'; message: { dongleFwVersion?: string; boxInfo?: unknown } }
  | { type: 'unplugged'; message?: undefined }
  | { type: 'failure'; message?: undefined }
  | { type: 'audio'; message: AudioData }
  | { type: 'video'; message: VideoData }
  | { type: 'media'; message: MediaData }
  | { type: 'command'; message: Command }

export default class Projection {
  private _pairTimeout: NodeJS.Timeout | null = null
  private _frameInterval: ReturnType<typeof setInterval> | null = null
  private _config: DongleConfig
  public dongleDriver: DongleDriver

  public onmessage: ((ev: ProjectionMessage) => void) | null = null
  public onReconnectReady: (() => void) | null = null

  constructor(config: Partial<DongleConfig>) {
    this._config = Object.assign({}, DEFAULT_CONFIG, config)
    const mic = new Microphone()
    const driver = new DongleDriver()

    let currentMicDecodeType: number | null = null

    mic.on('data', (data) => {
      if (currentMicDecodeType == null) {
        return
      }
      driver.send(new SendAudio(data, currentMicDecodeType))
    })

    driver.on('message', (message: Message) => {
      if (message instanceof Plugged) {
        this.clearPairTimeout()
        this.clearFrameInterval()
        const phoneTypeConfg = this._config.phoneConfig?.[message.phoneType]
        if (phoneTypeConfg?.frameInterval) {
          this._frameInterval = setInterval(
            () => this.dongleDriver.send(new SendCommand('frame')),
            phoneTypeConfg.frameInterval
          )
        }
        this.onmessage?.({ type: 'plugged' })
      } else if (message instanceof Unplugged) {
        this.onmessage?.({ type: 'unplugged' })
      } else if (message instanceof VideoData) {
        this.clearPairTimeout()
        this.onmessage?.({ type: 'video', message })
      } else if (message instanceof AudioData) {
        this.clearPairTimeout()
        this.onmessage?.({ type: 'audio', message })
      } else if (message instanceof MediaData) {
        this.clearPairTimeout()
        this.onmessage?.({ type: 'media', message })
      } else if (message instanceof Command) {
        this.onmessage?.({ type: 'command', message })
      }

      if (message instanceof AudioData && message.command != null) {
        if (message.command === AudioCommand.AudioInputConfig && message.decodeType != null) {
          currentMicDecodeType = message.decodeType
        }

        switch (message.command) {
          case AudioCommand.AudioVoiceAssistantStart:
          case AudioCommand.AudioPhonecallStart:
            if (currentMicDecodeType != null) {
              mic.start(currentMicDecodeType)
            }
            break
          case AudioCommand.AudioVoiceAssistantStop:
          case AudioCommand.AudioPhonecallStop:
            mic.stop()
            break
        }
      }
    })

    driver.on('failure', () => {
      this.onmessage?.({ type: 'failure' })
    })

    driver.on('dongle-info', (info: { dongleFwVersion?: string; boxInfo?: unknown }) => {
      this.onmessage?.({ type: 'dongleInfo', message: info })
    })

    this.dongleDriver = driver
  }

  private async findDevice(): Promise<ReturnType<typeof webusb.requestDevice> | null> {
    try {
      return await webusb.requestDevice({ filters: DongleDriver.knownDevices })
    } catch {
      return null
    }
  }

  public async resetDongle() {
    const device = await this.findDevice()
    if (!device) throw new Error('No dongle found for reset')
    await device.open()
    await device.reset()
    await device.close()
    console.log('[Projection] Dongle has been reset, waiting for reconnect...')
  }

  public async initialiseAfterReconnect() {
    const device = await this.findDevice()
    if (!device) throw new Error('Dongle not found after reconnect')
    await device.open()
    const { initialise, start, send } = this.dongleDriver
    await initialise(device)
    await start(this._config)
    this._pairTimeout = setTimeout(() => {
      console.debug('No device, sending wifiPair')
      send(new SendCommand('wifiPair'))
    }, 15000)
  }

  stop = async () => {
    try {
      this.clearPairTimeout()
      this.clearFrameInterval()
      await this.dongleDriver.close()
    } catch (err) {
      console.error(err)
    }
  }

  private clearPairTimeout() {
    if (this._pairTimeout) {
      clearTimeout(this._pairTimeout)
      this._pairTimeout = null
    }
  }

  private clearFrameInterval() {
    if (this._frameInterval) {
      clearInterval(this._frameInterval)
      this._frameInterval = null
    }
  }

  sendKey = (action: CommandValue) => {
    this.dongleDriver.send(new SendCommand(action))
  }

  sendTouch = ({ type, x, y }: { type: number; x: number; y: number }) => {
    this.dongleDriver.send(new SendTouch(x, y, type))
  }
}
