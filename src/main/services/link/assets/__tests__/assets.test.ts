import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { buildServerCgiScript } from '../LIVI_cgi.js'
import { buildLiviWeb } from '../LIVI_web.js'

describe('LIVI dongle web tools', () => {
  test('server.cgi is a shell script that routes actions', () => {
    const cgi = buildServerCgiScript()
    expect(cgi.startsWith('#!/bin/sh')).toBe(true)
    expect(cgi).toContain('Content-type')
    expect(cgi).toContain('$SCRIPT_NAME')
    expect(cgi).toContain('esac')
  })

  test('server.cgi parses as a shell script', () => {
    // The script is assembled from quoted TS strings, where a lost backslash silently changes
    // what the shell sees; sh -n catches that before the dongle does.
    const file = join(mkdtempSync(join(tmpdir(), 'livi-cgi-')), 'server.cgi')
    writeFileSync(file, buildServerCgiScript())
    expect(() => execFileSync('sh', ['-n', file])).not.toThrow()
  })

  test('the page script parses', () => {
    // The page carries its own JavaScript, and a broken string there disables every button
    // without any visible error.
    const script = buildLiviWeb().match(/<script>([\s\S]*?)<\/script>/)?.[1] ?? ''
    expect(script.length).toBeGreaterThan(100)
    expect(() => new Function(script)).not.toThrow()
  })

  test('index.html is a full document', () => {
    const html = buildLiviWeb()
    expect(html.startsWith('<!doctype html>')).toBe(true)
    expect(html).toContain('</html>')
  })

  test('an image upload is a raw body, bounded by CONTENT_LENGTH', () => {
    const cgi = buildServerCgiScript()
    expect(cgi).toContain('upload_image)')
    // No multipart parser in sh: the page POSTs the file as the body.
    expect(cgi).toContain('head -c "$n" > /tmp/restore.img')
    expect(cgi).toContain('sha256sum /tmp/restore.img')
    const html = buildLiviWeb()
    expect(html).toContain('restore()')
    expect(html).toContain('method: "POST"')
  })

  test('flashing is delegated to the dongle script, not reimplemented here', () => {
    const cgi = buildServerCgiScript()
    const body = cgi.slice(cgi.indexOf('flash_image() {'), cgi.indexOf('stop_livi_link() {'))
    // The guards and the erase live in /script/livi/flash-image.sh; this only calls it.
    // The partition is named here, so no query string can point the write somewhere else.
    expect(body).toContain('sh "$s" rootfs "$sha"')
    expect(body).toContain('provision the dongle first')
    expect(body).not.toContain('flash_erase')
  })

  test('the page insists on a real hash before it offers to flash', () => {
    const html = buildLiviWeb()
    expect(html).toContain('restore()')
    expect(html).toContain('/^[0-9a-f]{64}$/')
    // The hash comes from hashing the picked file, never from a text field.
    expect(html).not.toContain('placeholder="sha256')
    expect(html).toContain('do not unplug the dongle')
  })

  test('the page hashes files the same way the dongle does', () => {
    // crypto.subtle is unavailable over plain HTTP, so the page carries its own SHA-256; if it
    // were wrong, every restore would be refused for a hash mismatch.
    const html = buildLiviWeb()
    const script = html.slice(html.indexOf('const K256'), html.indexOf('async function hashPicked'))
    const sha256Hex = new Function(`${script}; return sha256Hex`)() as (b: ArrayBuffer) => string
    for (const n of [0, 1, 55, 56, 63, 64, 65, 1000, 65536]) {
      const bytes = Buffer.from(Array.from({ length: n }, (_, i) => (i * 31) % 251))
      expect(sha256Hex(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + n))).toBe(
        createHash('sha256').update(bytes).digest('hex')
      )
    }
  })

  test('the page can start the shell a stock dongle does not have', () => {
    const html = buildLiviWeb()
    expect(html).toContain('startShell()')
    expect(html).toContain('busybox telnetd -l /bin/sh -p 2323')
    expect(html).toContain('b.disabled = running')
  })

  test('restore is the only way back, and promises nothing about the image', () => {
    const html = buildLiviWeb()
    expect(html).toContain('restore()')
    expect(html).not.toContain('CarlinKit mode')
    // Installing the tools persistently is the provisioner's job, not a button here.
    expect(html).not.toContain('installPersistentWeb')
    // Booting the vendor init was a dead end: boa comes up from the bring-up it would disable.
    expect(html).not.toContain('stopLiviLink')
    expect(buildServerCgiScript()).not.toContain('stop_livi_link')
  })
})
