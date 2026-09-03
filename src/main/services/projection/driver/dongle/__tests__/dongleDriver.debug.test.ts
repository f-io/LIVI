vi.mock('@main/helpers/vendorSessionInfo', () => ({
  decryptVendorSessionText: vi.fn(async () => 'decrypted-session')
}))

const ORIG_DEBUG = process.env.DEBUG

type DriverModule = typeof import('../dongleDriver')
type InputModule = typeof import('@shared/types/InputCommand')

let DongleDriver: DriverModule['DongleDriver']
let InputCommand: InputModule['InputCommand']

beforeAll(async () => {
  process.env.DEBUG = '1'
  vi.resetModules()
  ;({ DongleDriver } = await import('../dongleDriver'))
  ;({ InputCommand } = await import('@shared/types/InputCommand'))
})

afterAll(() => {
  if (ORIG_DEBUG === undefined) delete process.env.DEBUG
  else process.env.DEBUG = ORIG_DEBUG
  vi.resetModules()
})

beforeEach(() => {
  vi.spyOn(console, 'log').mockImplementation(() => {})
})
afterEach(() => vi.restoreAllMocks())

describe('DongleDriver handleInput under DEBUG', () => {
  test('logs a diagnostic when there is no dongle mapping', () => {
    const d = new DongleDriver() as unknown as { handleInput: (c: unknown) => void }
    d.handleInput('bogus')
    expect(console.log).toHaveBeenCalled()
  })

  test('a mapped command still dispatches under DEBUG', () => {
    const d = new DongleDriver() as unknown as {
      handleInput: (c: unknown) => void
      send: (m: unknown) => Promise<boolean>
    }
    const send = vi.fn(async () => true)
    d.send = send
    d.handleInput(InputCommand.Play)
    expect(send).toHaveBeenCalled()
  })
})
