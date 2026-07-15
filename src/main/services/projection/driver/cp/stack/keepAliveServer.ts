import dgram from 'node:dgram'

export class KeepAliveServer {
  private _sock: dgram.Socket | null = null

  listen(): Promise<number> {
    return new Promise((resolve, reject) => {
      const sock = dgram.createSocket({ type: 'udp6', ipv6Only: false })
      sock.on('error', reject)
      sock.on('message', () => {})
      sock.bind(0, '::', () => {
        this._sock = sock
        resolve(sock.address().port)
      })
    })
  }

  stop(): void {
    this._sock?.close()
    this._sock = null
  }
}
