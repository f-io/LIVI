import { BACKUP_DIR, CONFIG_BACKUP_PATH, CONFIG_PATH } from '@main/config/paths'
import { homedir } from 'os'
import { join } from 'path'

describe('CONFIG_PATH', () => {
  test('points to config.json inside app userData', () => {
    expect(CONFIG_PATH).toBe('/tmp/config.json')
  })
})

describe('BACKUP_DIR', () => {
  test('is the one folder to carry to a new machine', () => {
    const expected =
      process.platform === 'darwin'
        ? '/tmp/backup'
        : join(process.env.XDG_DATA_HOME || join(homedir(), '.local', 'share'), 'LIVI')
    expect(BACKUP_DIR).toBe(expected)
    expect(CONFIG_BACKUP_PATH).toBe(join(expected, 'config.json'))
  })

  test('never coincides with the live config', () => {
    expect(CONFIG_BACKUP_PATH).not.toBe(CONFIG_PATH)
  })
})
