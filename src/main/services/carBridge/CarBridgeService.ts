import type { NavLocale } from '@shared/utils'
import { navIdleText, translateNavigation } from '@shared/utils'
import { execFile } from 'child_process'
import { app } from 'electron'
import fs from 'fs'
import path from 'path'
import type { ProjectionEvent } from '../projection/services/types'

// Pushes now-playing and turn-by-turn lines to the LIVI Bridge
const USB_VID = 'cafe' // TinyUSB test VID
const CDC_DATA_INTERFACE = '02'
const RETRY_MS = 3000

function navLocale(v: unknown): NavLocale {
  if (v === 'de') return 'de'
  if (v === 'fr') return 'fr'
  if (v === 'ua' || v === 'uk' || v === 'uk-UA') return 'ua'
  return 'en'
}

export class CarBridgeService {
  // HU panel keys arrive as "EV key ..." lines; forwarded as InputCommand
  // strings for ProjectionService.dispatchRemoteInput
  public onKey?: (command: string) => void

  // vehicle data lines ("car <Key> <value>")
  public onTelemetry?: (payload: Record<string, unknown>) => void

  private readonly locale: NavLocale
  private stream: fs.WriteStream | null = null
  private reader: fs.ReadStream | null = null
  private rxBuf = ''
  private retryTimer: NodeJS.Timeout | null = null
  private readonly rows = new Map<string, string>()
  private readonly lastSent = new Map<string, string>()
  private mediaBlankTimer: NodeJS.Timeout | null = null
  private stopped = false

  constructor(language: unknown) {
    this.locale = navLocale(language)
    this.rows.set('name', 'LIVI')
    this.rows.set('nav noRouteText', navIdleText(this.locale))
  }

  public start(): void {
    this.stopped = false
    this.connect()
  }

  public stop(): void {
    this.stopped = true
    if (this.retryTimer) clearTimeout(this.retryTimer)
    this.retryTimer = null
    if (this.mediaBlankTimer) clearTimeout(this.mediaBlankTimer)
    this.mediaBlankTimer = null
    this.stream?.end()
    this.stream = null
    this.reader?.destroy()
    this.reader = null
  }

  public handleEvent(event: ProjectionEvent): void {
    switch (event.type) {
      case 'media': {
        const media = event.payload.payload.media
        if (!media) return
        if (this.mediaBlankTimer) {
          clearTimeout(this.mediaBlankTimer)
          this.mediaBlankTimer = null
        }
        this.sendMediaFields(media as Record<string, unknown>)
        return
      }
      case 'media-reset':
        // resets fire on every session hop (start/idle/switch)
        if (this.mediaBlankTimer) clearTimeout(this.mediaBlankTimer)
        this.mediaBlankTimer = setTimeout(() => {
          this.mediaBlankTimer = null
          for (const key of CarBridgeService.MEDIA_TEXT_FIELDS) this.send(`media ${key}`, '')
        }, 4000)
        return
      case 'navigation': {
        const navi = event.payload.navi
        if (!navi) return
        this.sendNavFields(navi)
        return
      }
      case 'navigation-reset':
        this.send('nav NaviStatus', '0')
        return
      default:
        return
    }
  }

  private navFromDisk(): Record<string, unknown> | null {
    try {
      const raw = fs.readFileSync(path.join(app.getPath('userData'), 'navigationData.json'), 'utf8')
      const navi = JSON.parse(raw)?.payload?.navi
      return navi && typeof navi === 'object' ? navi : null
    } catch {
      return null
    }
  }

  // raw fields 1:1 plus the translated strings
  private sendNavFields(navi: unknown): void {
    const bag = navi as Record<string, unknown>
    for (const key of CarBridgeService.NAV_NUM_FIELDS) {
      const v = bag[key]
      if (typeof v === 'number') this.send(`nav ${key}`, String(v))
    }
    for (const key of CarBridgeService.NAV_TEXT_FIELDS) {
      const v = bag[key]
      if (typeof v === 'string' && v.trim()) this.send(`nav ${key}`, v.trim())
    }
    const translated = translateNavigation(navi as never, this.locale)
    const texts: Record<string, unknown> = {
      remainDistanceText: translated?.RemainDistanceText,
      maneuverText: translated?.ManeuverTypeText
    }
    for (const [key, v] of Object.entries(texts)) {
      if (typeof v === 'string' && v.trim()) this.send(`nav ${key}`, v.trim())
    }
  }

  private handleRx(chunk: string): void {
    this.rxBuf = (this.rxBuf + chunk).slice(-512)
    for (;;) {
      const nl = this.rxBuf.indexOf('\n')
      if (nl < 0) return
      const line = this.rxBuf.slice(0, nl).trim()
      this.rxBuf = this.rxBuf.slice(nl + 1)
      if (line === 'EV key next') this.onKey?.('next')
      else if (line === 'EV key prev') this.onKey?.('previous')
      else if (line === 'EV key scan') this.onKey?.('playPause')
      else if (line.startsWith('car ')) this.handleCarLine(line.slice(4))
    }
  }

  private handleCarLine(rest: string): void {
    const sp = rest.indexOf(' ')
    if (sp <= 0) return
    const key = rest.slice(0, sp)
    if (!/^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z][A-Za-z0-9_]*)?$/.test(key)) return
    //handleRx trims the line, so a valid sp guarantees a non-empty value
    const raw = rest.slice(sp + 1).trim()
    const value: unknown =
      raw === 'true'
        ? true
        : raw === 'false'
          ? false
          : Number.isFinite(Number(raw))
            ? Number(raw)
            : raw
    const dot = key.indexOf('.')
    const payload =
      dot > 0 ? { [key.slice(0, dot)]: { [key.slice(dot + 1)]: value } } : { [key]: value }
    this.onTelemetry?.(payload)
  }

  private mediaFromDisk(): Record<string, unknown> | null {
    try {
      const raw = fs.readFileSync(path.join(app.getPath('userData'), 'mediaData.json'), 'utf8')
      const media = JSON.parse(raw)?.payload?.media
      return media && typeof media === 'object' ? media : null
    } catch {
      return null
    }
  }
  private static readonly NAV_NUM_FIELDS = [
    'NaviStatus',
    'NaviManeuverType',
    'NaviDistanceToDestination',
    'NaviTimeToDestination'
  ]
  private static readonly NAV_TEXT_FIELDS = ['NaviETA', 'NaviRoadName', 'NaviDestinationName']

  private static readonly MEDIA_TEXT_FIELDS = [
    'MediaSongName',
    'MediaArtistName',
    'MediaAlbumName',
    'MediaAPPName'
  ]
  private static readonly MEDIA_NUM_FIELDS = [
    'MediaSongDuration',
    'MediaSongPlayTime',
    'MediaPlayStatus'
  ]

  private sendMediaFields(media: Record<string, unknown>): void {
    for (const key of CarBridgeService.MEDIA_TEXT_FIELDS) {
      const v = media[key]
      if (typeof v === 'string' && v.trim()) this.send(`media ${key}`, v.trim())
    }
    for (const key of CarBridgeService.MEDIA_NUM_FIELDS) {
      const v = media[key]
      if (typeof v === 'number') this.send(`media ${key}`, String(v))
    }
  }

  private send(field: string, value: string): void {
    this.rows.set(field, value)
    this.flush(field)
  }

  private flush(field: string): void {
    if (!this.stream) return
    const ascii = this.rows.get(field) as string //flush only sees existing keys
    if (this.lastSent.get(field) === ascii) return
    this.lastSent.set(field, ascii)
    this.stream.write(`${field} ${ascii}\n`)
  }

  // Match by USB VID + CDC data interface via sysfs
  private findPortLinux(): string | null {
    let ttys: string[] = []
    try {
      ttys = fs.readdirSync('/sys/class/tty').filter((name) => name.startsWith('ttyACM'))
    } catch {
      return null
    }
    for (const tty of ttys) {
      try {
        const ifaceDir = fs.realpathSync(`/sys/class/tty/${tty}/device`)
        const iface = fs.readFileSync(path.join(ifaceDir, 'bInterfaceNumber'), 'utf8').trim()
        const dev = path.dirname(ifaceDir)
        const vid = fs.readFileSync(path.join(dev, 'idVendor'), 'utf8').trim()
        if (vid === USB_VID && iface === CDC_DATA_INTERFACE) return `/dev/${tty}`
      } catch {
        /* not a usb tty */
      }
    }
    return null
  }

  // macOS has no by-id links: probe each cu.usbmodem* with "ping" -> bridge answers "pong"
  private probeDarwin(ports: string[], done: (port: string | null) => void): void {
    const next = ports.shift()
    if (!next) {
      done(null)
      return
    }
    let settled = false
    const finish = (hit: boolean) => {
      if (settled) return
      settled = true
      reader.destroy()
      writer.end()
      if (hit) done(next)
      else this.probeDarwin(ports, done)
    }
    const reader = fs.createReadStream(next)
    const writer = fs.createWriteStream(next)
    const timer = setTimeout(() => finish(false), 700)
    reader.on('data', (chunk) => {
      if (chunk.toString().includes('pong')) {
        clearTimeout(timer)
        finish(true)
      }
    })
    reader.on('error', () => {
      clearTimeout(timer)
      finish(false)
    })
    writer.on('error', () => {
      clearTimeout(timer)
      finish(false)
    })
    writer.on('open', () => writer.write('ping\n'))
  }

  private findPortAsync(done: (port: string | null) => void): void {
    if (process.platform === 'darwin') {
      let candidates: string[] = []
      try {
        candidates = fs
          .readdirSync('/dev')
          .filter((name) => name.startsWith('cu.usbmodem'))
          .map((name) => `/dev/${name}`)
      } catch {
        /* keep empty */
      }
      this.probeDarwin(candidates, done)
      return
    }
    done(this.findPortLinux())
  }

  private scheduleRetry(): void {
    if (this.stopped || this.retryTimer) return
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null
      this.connect()
    }, RETRY_MS)
  }

  private connect(): void {
    this.findPortAsync((port) => {
      if (this.stopped) return
      if (!port) {
        this.scheduleRetry()
        return
      }
      this.openPort(port)
    })
  }

  private openPort(port: string): void {
    // raw mode without echo
    const sttyArgs =
      process.platform === 'darwin'
        ? ['-f', port, 'raw', '-echo', '115200']
        : ['-F', port, 'raw', '-echo', '115200']
    execFile('stty', sttyArgs, (err) => {
      if (this.stopped) return
      if (err && process.platform !== 'darwin') {
        this.scheduleRetry()
        return
      }
      const stream = fs.createWriteStream(port, { flags: 'w' })
      stream.on('error', () => {
        this.stream = null
        this.lastSent.clear()
        this.reader?.destroy()
        this.reader = null
        this.scheduleRetry()
      })
      stream.on('open', () => {
        console.log(`[CarBridge] connected to ${port}`)
        this.stream = stream
        this.lastSent.clear()
        if (![...this.rows.keys()].some((k) => k.startsWith('media '))) {
          const disk = this.mediaFromDisk()
          if (disk) this.sendMediaFields(disk)
        }
        if (![...this.rows.keys()].some((k) => k.startsWith('nav Navi'))) {
          const disk = this.navFromDisk()
          if (disk) this.sendNavFields(disk)
        }
        for (const field of this.rows.keys()) this.flush(field)
      })
      stream.on('close', () => {
        if (this.stream === stream) this.stream = null
        this.reader?.destroy()
        this.reader = null
        this.scheduleRetry()
      })
      const reader = fs.createReadStream(port)
      reader.on('data', (chunk) => this.handleRx(chunk.toString()))
      // A dead reader means a dead port, drop the writer so the usual
      // error/close path reconnects both
      reader.on('error', () => stream.destroy())
      this.reader = reader
    })
  }
}
