import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { parseManifest, requiredPackages } from '../services/packageCheck'

const MANIFEST = readFileSync(join(process.cwd(), 'scripts/install/packages.txt'), 'utf8')

describe('parseManifest', () => {
  it('skips comments and blank lines', () => {
    expect(parseManifest('# a comment\n\n   \n')).toEqual([])
  })

  it('reads section, name, probe and purpose', () => {
    expect(parseManifest('core|bluez|cmd:bluetoothctl|Bluetooth pairing')).toEqual([
      { section: 'core', name: 'bluez', probe: 'cmd:bluetoothctl', purpose: 'Bluetooth pairing' }
    ])
  })

  it('tolerates a missing purpose', () => {
    expect(parseManifest('lite|cage|cmd:cage|')).toEqual([
      { section: 'lite', name: 'cage', probe: 'cmd:cage', purpose: '' }
    ])
  })

  it('drops lines with an unknown section, no package or no probe', () => {
    expect(parseManifest('bogus|x|cmd:x|y\ncore||cmd:x|y\ncore|x||y')).toEqual([])
  })
})

describe('the shipped manifest', () => {
  const entries = parseManifest(MANIFEST)

  it('parses every non-comment line', () => {
    const lines = MANIFEST.split('\n').filter((l) => l.trim() && !l.trim().startsWith('#'))
    expect(entries).toHaveLength(lines.length)
  })

  it('gives every package a purpose, so the prompt can explain itself', () => {
    for (const e of entries) expect(e.purpose, `${e.name} has no purpose`).not.toBe('')
  })

  it('lists no package twice', () => {
    const names = entries.map((e) => e.name)
    expect(new Set(names).size).toBe(names.length)
  })

  it('gives every package a probe kind the checker understands', () => {
    for (const e of entries) {
      expect(e.probe, `${e.name} has no probe`).toMatch(/^(cmd|py|gst):.+/)
    }
  })

  it('keeps avahi in core, since pi-lite needs the daemon too', () => {
    const avahi = entries.filter((e) => e.name.includes('avahi'))
    expect(avahi.map((e) => e.name).sort()).toEqual(['avahi-daemon'])
    for (const e of avahi) expect(e.section).toBe('core')
  })
})

describe('requiredPackages', () => {
  const entries = parseManifest('core|a|cmd:a|x\nlite|b|cmd:b|y')

  it('takes core plus lite when there is no desktop session', () => {
    const prev = process.env.XDG_CURRENT_DESKTOP
    delete process.env.XDG_CURRENT_DESKTOP
    expect(requiredPackages(entries).map((e) => e.name)).toEqual(['a', 'b'])
    if (prev !== undefined) process.env.XDG_CURRENT_DESKTOP = prev
  })

  it('takes core only on a desktop host', () => {
    const prev = process.env.XDG_CURRENT_DESKTOP
    process.env.XDG_CURRENT_DESKTOP = 'GNOME'
    expect(requiredPackages(entries).map((e) => e.name)).toEqual(['a'])
    if (prev === undefined) delete process.env.XDG_CURRENT_DESKTOP
    else process.env.XDG_CURRENT_DESKTOP = prev
  })
})
