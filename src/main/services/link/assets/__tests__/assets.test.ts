import { buildServerCgiScript } from '../LIVI_cgi.js'
import { buildLiviWeb } from '../LIVI_web.js'

describe('LIVI dongle web tools', () => {
  test('server.cgi is a shell script that routes actions', () => {
    const cgi = buildServerCgiScript()
    expect(cgi.startsWith('#!/bin/sh')).toBe(true)
    expect(cgi).toContain('Content-type')
    expect(cgi).toContain('server.cgi')
    expect(cgi).toContain('esac')
  })

  test('index.html is a full document', () => {
    const html = buildLiviWeb()
    expect(html.startsWith('<!doctype html>')).toBe(true)
    expect(html).toContain('</html>')
  })
})
