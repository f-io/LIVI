import { webusb } from 'usb'
import { Microphone } from '@audio'

import {
  AudioData,
  MediaData,
  Message,
  Plugged,
  SendAudio,
  SendCommand,
  SendTouch,
  Unplugged,
  VideoData,
  CommandValue,
  Command,
  AudioCommand
} from '../messages'

import { DongleDriver, DongleConfig, DEFAULT_CONFIG } from '../driver/DongleDriver'

export type CarplayMessage =
  | { type: 'plugged'; message?: undefined }
  | { type: 'dongleInfo'; message: { dongleFwVersion?: string; boxInfo?: unknown } }
  | { type: 'unplugged'; message?: undefined }
  | { type: 'failure'; message?: undefined }
  | { type: 'audio'; message: AudioData }
  | { type: 'video'; message: VideoData }
  | { type: 'media'; message: MediaData }
  | { type: 'command'; message: Command }

export default class Carplay {
  private _pairTimeout: NodeJS.Timeout | null = null
  private _frameInterval: ReturnType<typeof setInterval> | null = null
  private _config: DongleConfig
  public dongleDriver: DongleDriver

  public onmessage: ((ev: CarplayMessage) => void) | null = null
  public onReconnectReady: (() => void) | null = null

  constructor(config: Partial<DongleConfig>) {
    this._config = Object.assign({}, DEFAULT_CONFIG, config)
    const mic = new Microphone()
    const driver = new DongleDriver()

    mic.on('data', (data) => {
      driver.send(new SendAudio(data))
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
        switch (message.command) {
          case AudioCommand.AudioSiriStart:
          case AudioCommand.AudioPhonecallStart:
            mic.start()
            break
          case AudioCommand.AudioSiriStop:
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

  private async findDevice() {
    let device: USBDevice | null = null
    try {
      device = await webusb.requestDevice({ filters: DongleDriver.knownDevices })
    } catch (_) {
      return null
    }
    return device
  }

  public async resetDongle() {
    const device = await this.findDevice()
    if (!device) throw new Error('No dongle found for reset')
    await device.open()
    await device.reset()
    await device.close()
    console.log('[Carplay] Dongle has been reset, waiting for reconnect...')
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
