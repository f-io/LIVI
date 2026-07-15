/**
 * rtspMessage — RTSP/HTTP-style request framing for the CarPlay control channel.
 *
 * The control connection on TCP :7000 speaks a text request line + headers +
 * optional binary-plist body, same framing as RTSP and HTTP share. This parses
 * incoming requests incrementally and builds responses. Encryption (once the
 * handshake completes) wraps this framing in a separate layer.
 */

export interface RtspRequest {
  method: string
  path: string
  protocol: string
  /** Header names lower-cased. */
  headers: Record<string, string>
  body: Buffer
}

export interface RtspResponse {
  protocol?: string
  status?: number
  statusText?: string
  headers?: Record<string, string>
  body?: Buffer
}

const HEADER_END = Buffer.from('\r\n\r\n')

/**
 * Parse as many complete messages as are buffered. Returns the parsed messages
 * and the unconsumed remainder (a partial message still being received).
 */
export function parseMessages(buf: Buffer): { messages: RtspRequest[]; rest: Buffer } {
  const messages: RtspRequest[] = []
  let offset = 0

  while (offset < buf.length) {
    const headerEnd = buf.indexOf(HEADER_END, offset)
    if (headerEnd === -1) break

    const headerText = buf.toString('ascii', offset, headerEnd)
    const lines = headerText.split('\r\n')
    const requestLine = lines.shift() ?? ''
    const [method, path, protocol] = requestLine.split(' ')

    const headers: Record<string, string> = {}
    for (const line of lines) {
      const idx = line.indexOf(':')
      if (idx === -1) continue
      headers[line.slice(0, idx).trim().toLowerCase()] = line.slice(idx + 1).trim()
    }

    const bodyStart = headerEnd + HEADER_END.length
    const contentLength = Number.parseInt(headers['content-length'] ?? '0', 10) || 0
    const bodyEnd = bodyStart + contentLength
    if (bodyEnd > buf.length) break // body not fully received yet

    messages.push({
      method: method ?? '',
      path: path ?? '',
      protocol: protocol ?? 'RTSP/1.0',
      headers,
      body: Buffer.from(buf.subarray(bodyStart, bodyEnd))
    })
    offset = bodyEnd
  }

  return { messages, rest: Buffer.from(buf.subarray(offset)) }
}

const STATUS_TEXT: Record<number, string> = {
  200: 'OK',
  400: 'Bad Request',
  404: 'Not Found',
  500: 'Internal Server Error'
}

/** Build a response, echoing the request protocol and its CSeq. */
export function buildResponse(req: RtspRequest, res: RtspResponse): Buffer {
  const protocol = res.protocol ?? req.protocol ?? 'RTSP/1.0'
  const status = res.status ?? 200
  const statusText = res.statusText ?? STATUS_TEXT[status] ?? 'OK'
  const body = res.body ?? Buffer.alloc(0)

  const headers: Record<string, string> = { ...res.headers }
  if (req.headers.cseq != null) headers.CSeq = req.headers.cseq
  headers['Content-Length'] = String(body.length)

  const head =
    `${protocol} ${status} ${statusText}\r\n` +
    Object.entries(headers)
      .map(([k, v]) => `${k}: ${v}\r\n`)
      .join('') +
    '\r\n'

  return Buffer.concat([Buffer.from(head, 'ascii'), body])
}
