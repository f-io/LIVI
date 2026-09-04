import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { parseManifest, requiredPackages } from '../services/packageCheck'

const MANIFEST = readFileSync(join(process.cwd(), 'scripts/install/packages.txt'), 'utf8')

describe('parseManifest', () => {
  it('skips comments and blank lines', () => {
    expect(parseManifest('# a comment\n\n   \n')).toEqual([])
  })

  it('reads section, name, probe, purpose and the fedora name', () => {
    expect(parseManifest('core|dnsmasq-base|cmd:dnsmasq|DHCP|dnsmasq')).toEqual([
      {
        section: 'core',
        name: 'dnsmasq-base',
        probe: 'cmd:dnsmasq',
        purpose: 'DHCP',
        fedora: 'dnsmasq'
      }
    ])
  })

  it('tolerates a missing purpose and fedora name', () => {
    expect(parseManifest('lite|cage|cmd:cage|')).toEqual([
      { section: 'lite', name: 'cage', probe: 'cmd:cage', purpose: '', fedora: '' }
    ])
  })

  it('tolerates an absent purpose field', () => {
    expect(parseManifest('core|bluez|cmd:bluetoothctl')).toEqual([
      { section: 'core', name: 'bluez', probe: 'cmd:bluetoothctl', purpose: '', fedora: '' }
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
      expect(e.probe, `${e.name} has no probe`).toMatch(/^(cmd|py|gst|file):.+/)
    }
  })

  it('keeps avahi in core, since pi-lite needs the daemon and the helper spawns the tools', () => {
    const avahi = entries.filter((e) => e.name.includes('avahi'))
    expect(avahi.map((e) => e.name).sort()).toEqual(['avahi-daemon', 'avahi-utils'])
    for (const e of avahi) expect(e.section).toBe('core')
  })
})

describe('requiredPackages', () => {
  const entries = parseManifest('core|a|cmd:a|x\nlite|b|cmd:b|y')
  const SESSION_VARS = ['XDG_CURRENT_DESKTOP', 'WAYLAND_DISPLAY', 'DISPLAY'] as const
  let saved: Record<string, string | undefined>

  beforeEach(() => {
    saved = Object.fromEntries(SESSION_VARS.map((v) => [v, process.env[v]]))
    for (const v of SESSION_VARS) delete process.env[v]
  })

  afterEach(() => {
    for (const v of SESSION_VARS) {
      if (saved[v] === undefined) delete process.env[v]
      else process.env[v] = saved[v]
    }
  })

  it('takes core plus lite when there is no desktop session', () => {
    expect(requiredPackages(entries).map((e) => e.name)).toEqual(['a', 'b'])
  })

  it('takes core only on a desktop host', () => {
    process.env.XDG_CURRENT_DESKTOP = 'GNOME'
    expect(requiredPackages(entries).map((e) => e.name)).toEqual(['a'])
  })

  it('a wayland session counts as a desktop even without XDG_CURRENT_DESKTOP', () => {
    process.env.WAYLAND_DISPLAY = 'wayland-0'
    expect(requiredPackages(entries).map((e) => e.name)).toEqual(['a'])
  })

  it('an x11 session counts as a desktop too', () => {
    process.env.DISPLAY = ':0'
    expect(requiredPackages(entries).map((e) => e.name)).toEqual(['a'])
  })
})
