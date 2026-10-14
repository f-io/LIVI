import type { Action, FromCore, Front, Input, Screen, State, ToCore } from './contract'
import { applyOps } from './patch'

/** False when core is not connected. */
export type Send = (msg: ToCore) => boolean

type Listener = (state: State, prev: State | null) => void

type SpectrumListener = (bands: number[]) => void

type PageListener = (path: string) => void

type Waiting = { resolve: () => void; reject: (e: Error) => void }

export class CoreSession {
  state: State | null = null
  private rev = 0
  private nextId = 1
  private readonly waiting = new Map<number, Waiting>()
  private readonly listeners = new Set<Listener>()
  private lastPath: string | null = null
  private readonly shown = new Map<Screen, Front>()
  private drawsSpectrum = false
  private showsLinkSpeed = false
  private cameraFound: boolean | null = null
  private readonly spectrumListeners = new Set<SpectrumListener>()
  private readonly pageListeners = new Set<PageListener>()

  constructor(private readonly send: Send) {}

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  onSpectrum(listener: SpectrumListener): () => void {
    this.spectrumListeners.add(listener)
    return () => this.spectrumListeners.delete(listener)
  }

  onOpenPage(listener: PageListener): () => void {
    this.pageListeners.add(listener)
    return () => this.pageListeners.delete(listener)
  }

  receive(msg: FromCore): void {
    switch (msg.type) {
      case 'welcome':
        this.update(msg.state, msg.rev)
        if (this.lastPath !== null) this.send({ type: 'path', path: this.lastPath })
        for (const [screen, front] of this.shown) this.tellShown(screen, front)
        if (this.drawsSpectrum) this.send({ type: 'spectrum', on: true })
        if (this.showsLinkSpeed) this.send({ type: 'linkSpeed', on: true })
        if (this.cameraFound !== null) this.send({ type: 'camera', found: this.cameraFound })
        return
      case 'spectrum':
        for (const listener of this.spectrumListeners) listener(msg.bands)
        return
      case 'openPage':
        for (const listener of this.pageListeners) listener(msg.path)
        return
      case 'patch': {
        if (!this.state || msg.rev !== this.rev + 1) {
          this.resync()
          return
        }
        let next: State
        try {
          next = applyOps(this.state, msg.ops)
        } catch {
          this.resync()
          return
        }
        this.update(next, msg.rev)
        return
      }
      case 'reply': {
        const waiting = this.waiting.get(msg.id)
        if (!waiting) return
        this.waiting.delete(msg.id)
        if (msg.error === undefined) waiting.resolve()
        else waiting.reject(new Error(msg.error))
        return
      }
      case 'refused':
        console.error(`[core] refused: ${msg.reason}`)
    }
  }

  disconnected(): void {
    for (const waiting of this.waiting.values()) waiting.reject(new Error('core went away'))
    this.waiting.clear()
  }

  act(action: Action): Promise<void> {
    const id = this.nextId++
    return new Promise<void>((resolve, reject) => {
      this.waiting.set(id, { resolve, reject })
      if (!this.send({ type: 'action', id, action })) {
        this.waiting.delete(id)
        reject(new Error('core is not connected'))
      }
    })
  }

  resync(): void {
    this.send({ type: 'resync' })
  }

  /** Kept and sent again after a reconnect, core may have missed it. */
  path(path: string): void {
    this.lastPath = path
    this.send({ type: 'path', path })
  }

  /** Raw input is dropped while core is away, a late touch or key does harm. */
  input(input: Input): void {
    this.send({ type: 'input', input })
  }

  show(screen: Screen, front: Front): void {
    this.shown.set(screen, front)
    this.tellShown(screen, front)
  }

  spectrum(on: boolean): void {
    this.drawsSpectrum = on
    this.send({ type: 'spectrum', on })
  }

  linkSpeed(on: boolean): void {
    this.showsLinkSpeed = on
    this.send({ type: 'linkSpeed', on })
  }

  camera(found: boolean): void {
    this.cameraFound = found
    this.send({ type: 'camera', found })
  }

  private tellShown(screen: Screen, front: Front): void {
    this.act({ kind: 'show', screen, front }).catch(() => {})
  }

  private update(state: State, rev: number): void {
    const prev = this.state
    this.state = state
    this.rev = rev
    for (const listener of this.listeners) listener(state, prev)
  }
}
