import { EventEmitter } from 'node:events'

class MockAAStack extends EventEmitter {
  cfg: unknown
  stop = vi.fn()
  attachLink = vi.fn()
  setConfigRefresh = vi.fn()
  setClusterStreamActive = vi.fn()
  applyDisplayConfig = vi.fn()
  requestVideoFocus = vi.fn()
  requestMainKeyframe = vi.fn()
  requestClusterKeyframe = vi.fn()
  forceClusterKeyframe = vi.fn()
  requestShutdown = vi.fn(async () => undefined)
  sendTouch = vi.fn()
  sendButton = vi.fn()
  sendRotary = vi.fn()
  sendMicPcm = vi.fn()
  constructor(cfg: unknown) {
    super()
    this.cfg = cfg
  }
}

/** The helper's session link, as far as the mocked stack needs it. */
class FakeLink extends EventEmitter {
  readonly peer = '10.0.0.2'
  closed = false
  send = vi.fn()
  control = vi.fn()
  end = vi.fn()
  destroy = vi.fn()
}

const lastAaStack: { instance: MockAAStack | null } = { instance: null }

vi.mock('../stack/index', async () => {
  const real = await vi.importActual('../stack/index')
  return {
    ...real,
    AAStack: vi.fn().mockImplementation(function (cfg: unknown) {
      const aa = new MockAAStack(cfg)
      lastAaStack.instance = aa
      return aa
    })
  }
})

const ORIG_DEBUG = process.env.DEBUG

type SessionModule = typeof import('../AaSession')
type SendableModule = typeof import('../../../messages/sendable')
type EnumModule = typeof import('@shared/types/ProjectionEnums')
type InputModule = typeof import('@shared/types/InputCommand')

let AaSession: SessionModule['AaSession']
let SendCommand: SendableModule['SendCommand']
let SendTouch: SendableModule['SendTouch']
let MultiTouch: SendableModule['SendMultiTouch']
let TouchAction: EnumModule['TouchAction']
let InputCommand: InputModule['InputCommand']

beforeAll(async () => {
  process.env.DEBUG = '1'
  vi.resetModules()
  ;({ AaSession } = await import('../AaSession'))
  ;({
    SendCommand,
    SendTouch,
    SendMultiTouch: MultiTouch
  } = await import('../../../messages/sendable'))
  ;({ TouchAction } = await import('@shared/types/ProjectionEnums'))
  ;({ InputCommand } = await import('@shared/types/InputCommand'))
})

afterAll(() => {
  if (ORIG_DEBUG === undefined) delete process.env.DEBUG
  else process.env.DEBUG = ORIG_DEBUG
  vi.resetModules()
})

beforeEach(() => {
  lastAaStack.instance = null
  vi.spyOn(console, 'log').mockImplementation(() => {})
  vi.spyOn(console, 'warn').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => vi.restoreAllMocks())

const baseCfg = () =>
  ({
    projectionWidth: 1280,
    projectionHeight: 720,
    projectionFps: 30,
    projectionDpi: 0,
    hand: 0,
    carName: 'LIVI',
    wifiPassword: 'pw',
    wifiChannel: 36,
    clusterWidth: 800,
    clusterHeight: 480,
    clusterFps: 30,
    clusterDpi: 0,
    disableAudioOutput: false
  }) as unknown as import('@shared/types').Config

function make() {
  return new AaSession({
    transport: new FakeLink() as never,
    getConfig: () => baseCfg(),
    wired: false,
    seed: {
      hevcSupported: false,
      vp9Supported: false,
      av1Supported: false,
      initialNightMode: undefined,
      clusterStreamActive: true
    }
  })
}

describe('AaSession input logging under DEBUG', () => {
  test('every input command path logs and dispatches', async () => {
    const d = make()
    for (const cmd of [
      'selectDown',
      'selectUp',
      'knobDown',
      'knobUp',
      'voiceAssistant',
      'voiceAssistantRelease',
      'left',
      'right',
      'up',
      'down',
      'home',
      'play'
    ] as const) {
      await d.send(new SendCommand(cmd))
    }
    await d.send(new SendTouch(0.5, 0.5, TouchAction.Down))
    await d.send(new MultiTouch([{ id: 0, x: 0.5, y: 0.5, action: 0 as never }]))
    expect(lastAaStack.instance!.sendButton).toHaveBeenCalled()
  })

  test('an unmapped command still logs its no-key path', async () => {
    const d = make()
    const cmd = new SendCommand('home')
    ;(cmd as { value: number }).value = 0xffffff
    await d.send(cmd)
    expect(console.log).toHaveBeenCalled()
  })

  test('handleInput logs when there is no AA mapping', () => {
    const d = make()
    d.handleInput('nope' as never)
    expect(console.log).toHaveBeenCalled()
  })

  test('handleInput dispatches a mapped command', () => {
    const d = make()
    d.handleInput(InputCommand.VolumeUp)
    expect(lastAaStack.instance!.sendButton).toHaveBeenCalled()
  })
})
