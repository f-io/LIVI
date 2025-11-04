// Based on https://github.com/OllieJones/h264-interp-utils
// MIT License

function byte2hex(val: number) {
  return ('00' + val.toString(16)).slice(-2)
}

const validNaluTypes = new Set(['packet', 'annexB', 'unknown'])

export const profileNames: Map<number, string> = new Map([
  [66, 'BASELINE'],
  [77, 'MAIN'],
  [88, 'EXTENDED'],
  [100, 'FREXT_HP'],
  [110, 'FREXT_Hi10P'],
  [122, 'FREXT_Hi422'],
  [244, 'FREXT_Hi444'],
  [44, 'FREXT_CAVLC444']
])

export const chromaFormatValues = {
  0: 'YUV400',
  1: 'YUV420',
  2: 'YUV422',
  3: 'YUV444'
}

// noinspection DuplicatedCode
/**
 * Tools for handling general bitstream issues.
 */
export class RawBitstream {
  ptr: number
  buffer: Uint8Array
  protected originalByteLength: number
  protected max: number

  /**
   * Construct a bitstream
   * @param stream  Buffer containing the stream, or length in bits
   */
  constructor(stream: Uint8Array | number) {
    this.ptr = 0
    if (typeof stream === 'number') {
      this.buffer = new Uint8Array((stream + 7) >> 3)
      this.originalByteLength = this.buffer.byteLength
      this.max = stream
    } else if (typeof stream === 'undefined') {
      this.buffer = new Uint8Array(8192)
      this.originalByteLength = this.buffer.byteLength
      this.max = 8192 << 3
    } else {
      this.buffer = new Uint8Array(stream.buffer, stream.byteOffset, stream.byteLength)
      this.max = this.buffer.byteLength << 3
      this.originalByteLength = stream.byteLength
    }
  }

  /**
   * utility  / debugging function to examine next 16 bits of stream
   * @returns {string} Remaining unconsumed bits in the stream
   * (Careful: getters cannot have side-effects like advancing a pointer)
   */
  get peek16(): string {
    let n = 16
    let p = this.ptr
    if (n + p > this.remaining) n = this.remaining
    const bitstrings: string[] = []
    const hexstrings: string[] = []
    /* nibble accumulators */
    const bits: number[] = []
    let nibble = 0
    for (let i = 0; i < n; i++) {
      const q = p >> 3
      const o = 0x07 - (p & 0x07)
      const bit = (this.buffer[q]! >> o) & 0x01
      nibble = (nibble << 1) | bit
      bits.push(bit)
      p++
      if (i === n - 1 || i % 4 === 3) {
        hexstrings.push(nibble.toString(16))
        let bitstring = ''
        bits.forEach((bit) => {
          bitstring += bit === 0 ? '0' : '1'
        })
        bitstrings.push(bitstring)
        bits.length = 0
        nibble = 0
      }
    }
    return bitstrings.join(' ') + ' ' + hexstrings.join('')
  }

  /**
   * number of bits remaining in the present stream
   * @returns {number}
   */
  get remaining() {
    return this.max - this.ptr
  }

  /**
   * number of bits already consumed in the present stream
   * @returns {number}
   */
  get consumed() {
    return this.ptr
  }

  seek(pos = 0) {
    if (pos > this.max) throw new Error('cannot seek beyond end')
    this.ptr = pos
  }

  reallocate(size: number) {
    if (this.ptr + size <= this.max) return
    const newSize = (0xff + Math.floor((this.max + size) * 1.25)) & ~0xff
    const newBuf = new Uint8Array((newSize + 7) >>> 3)
    this.max = newSize
    newBuf.set(this.buffer)
    this.buffer = newBuf
  }

  /**
   * copy bits from some other bitstream to this one
   * @param {RawBitstream} from the source of the copy
   * @param {number} ptr the starting bit position of the copy in "from"
   * @param {number} count the number of bits to copy
   * @param {number|undefined} to the starting bit position to receive the copy, or the current pointer
   */
  copyBits(from: RawBitstream, ptr: number, count: number, to: number | undefined) {
    /* this is a little intricate for the sake of performance */
    this.reallocate(count)
    /* handle pointer saving. */
    const savedFromPtr = from.ptr
    const savedToPtr = this.ptr
    from.ptr = ptr
    if (typeof to === 'number') this.ptr = to

    /* split the copy into a starting fragment of < 8 bits,
     * a multiple of 8 bits,
     * and an ending fragment of less than 8 bits
     */
    const firstFragLen = (8 - this.ptr) & 0x07
    const lastFragLen = (count - firstFragLen) & 0x07
    const byteCopyLen = count - (firstFragLen + lastFragLen)

    /* copy first fragment bit by bit */
    for (let i = 0; i < firstFragLen; i++) {
      const b = from.u_1()
      this.put_u_1(b)
    }

    /* copy whole bytes byte-by-byte */
    const thisbuf = this.buffer
    const frombuf = from.buffer
    let q = this.ptr >> 3
    const byteLen = byteCopyLen >> 3
    const lshift = from.ptr & 0x07
    if (lshift === 0) {
      /* byte-aligned source and dest */
      let r = from.ptr >> 3

      /* four-way loop unroll */
      let n = byteLen & 0x03
      while (n-- > 0) {
        thisbuf[q++] = frombuf[r++]!
      }
      n = byteLen >> 2
      while (n-- > 0) {
        thisbuf[q++] = frombuf[r++]!
        thisbuf[q++] = frombuf[r++]!
        thisbuf[q++] = frombuf[r++]!
        thisbuf[q++] = frombuf[r++]!
      }
    } else {
      /* unaligned source, retrieve it with masks and shifts */
      const rshift = 8 - lshift
      const mask = (0xff << rshift) & 0xff
      let p = (from.ptr >> 3) + 1
      let v1 = frombuf[p - 1]!
      let v2 = frombuf[p]!

      /* 8-way loop unroll.
       * This is a hot path when changing pic_parameter_set_ids in Slices. */
      let n = byteLen & 0x07
      while (n-- > 0) {
        thisbuf[q++] = ((v1 & ~mask) << lshift) | ((v2 & mask) >> rshift)
        v1 = v2
        v2 = frombuf[++p]!
      }
      n = byteLen >> 3
      while (n-- > 0) {
        /* flip back and forth between v1 and v2 */
        thisbuf[q++] = ((v1 & ~mask) << lshift) | ((v2 & mask) >> rshift)
        v1 = frombuf[++p]!

        thisbuf[q++] = ((v2 & ~mask) << lshift) | ((v1 & mask) >> rshift)
        v2 = frombuf[++p]!

        thisbuf[q++] = ((v1 & ~mask) << lshift) | ((v2 & mask) >> rshift)
        v1 = frombuf[++p]!

        thisbuf[q++] = ((v2 & ~mask) << lshift) | ((v1 & mask) >> rshift)
        v2 = frombuf[++p]!

        thisbuf[q++] = ((v1 & ~mask) << lshift) | ((v2 & mask) >> rshift)
        v1 = frombuf[++p]!

        thisbuf[q++] = ((v2 & ~mask) << lshift) | ((v1 & mask) >> rshift)
        v2 = frombuf[++p]!

        thisbuf[q++] = ((v1 & ~mask) << lshift) | ((v2 & mask) >> rshift)
        v1 = frombuf[++p]!

        thisbuf[q++] = ((v2 & ~mask) << lshift) | ((v1 & mask) >> rshift)
        v2 = frombuf[++p]!
      }
    }
    from.ptr += byteCopyLen
    this.ptr += byteCopyLen

    /* copy the last fragment bit by bit */
    for (let i = 0; i < lastFragLen; i++) {
      const b = from.u_1()
      this.put_u_1(b)
    }

    /* restore saved pointers */
    from.ptr = savedFromPtr
    if (typeof to === 'number') this.ptr = savedToPtr
  }

  /**
   * put one bit
   */
  put_u_1(b: number) {
    if (this.ptr + 1 > this.max) throw new Error('NALUStream error: bitstream exhausted')
    const p = this.ptr >> 3
    const o = 0x07 - (this.ptr & 0x07)
    const val = b << o
    const mask = ~(1 << o)
    this.buffer[p] = (this.buffer[p]! & mask) | val
    this.ptr++
    return val
  }

  /**
   * get one bit
   * @returns {number}
   */
  u_1() {
    if (this.ptr + 1 > this.max) throw new Error('NALUStream error: bitstream exhausted')
    const p = this.ptr >> 3
    const o = 0x07 - (this.ptr & 0x07)
    const val = (this.buffer[p]! >> o) & 0x01
    this.ptr++
    return val
  }

  /**
   * get two bits
   * @returns {number}
   */
  u_2() {
    return (this.u_1() << 1) | this.u_1()
  }

  /**
   * get three bits
   * @returns {number}
   */
  u_3() {
    return (this.u_1() << 2) | (this.u_1() << 1) | this.u_1()
  }

  /**
   * get n bits
   * @param n
   * @returns {number}
   */
  u(n: number) {
    if (n === 8) return this.u_8()
    if (this.ptr + n >= this.max) throw new Error('NALUStream error: bitstream exhausted')
    let val = 0
    for (let i = 0; i < n; i++) {
      val = (val << 1) | this.u_1()
    }
    return val
  }

  /**
   * get one byte (as an unsigned number)
   * @returns {number}
   */
  u_8() {
    if (this.ptr + 8 > this.max) throw new Error('NALUStream error: bitstream exhausted')
    const o = this.ptr & 0x07
    if (o === 0) {
      const val = this.buffer[this.ptr >> 3]
      this.ptr += 8
      return val
    } else {
      const n = 8 - o
      const rmask = (0xff << n) & 0xff
      const lmask = ~rmask & 0xff
      const p = this.ptr >> 3
      this.ptr += 8
      return ((this.buffer[p]! & lmask) << o) | ((this.buffer[p + 1]! & rmask) >> n)
    }
  }

  /**
   * get an unsigned H.264-style variable-bit number
   * in exponential Golomb format
   * @returns {number}
   */
  ue_v() {
    let zeros = 0
    while (!this.u_1()) zeros++
    let val = 1 << zeros
    for (let i = zeros - 1; i >= 0; i--) {
      val |= this.u_1() << i
    }
    return val - 1
  }

  put_u8(val: number) {
    this.reallocate(8)
    if ((this.ptr & 0x07) === 0) {
      this.buffer[this.ptr >> 3] = val
      this.ptr += 8
      return
    }
    this.put_u(val, 8)
  }

  put_u(val: number, count: number) {
    this.reallocate(count)
    if (count === 0) return
    while (count > 0) {
      count--
      this.put_u_1((val >> count) & 0x01)
    }
  }

  /**
   * Put an exponential-Golomb coded unsigned integer into the bitstream
   * https://en.wikipedia.org/wiki/Exponential-Golomb_coding
   * @param {number} val to insert
   * @returns {number} count of bits inserted
   */
  put_ue_v(val: number) {
    const v = val + 1
    let v1 = v
    let z = -1
    do {
      z++
      v1 = v1 >> 1
    } while (v1 !== 0)
    this.put_u(0, z)
    this.put_u(v, z + 1)
    return z + z + 1
  }

  /**
   * when done putting into a buffer, mark it complete,
   * rewind it to the beginning, and shorten its contents,
   * as if it had just been loaded.
   * @returns {number} the number of bits in the buffer
   */
  put_complete() {
    const newLength = this.ptr
    const newByteLength = (newLength + 7) >> 3
    this.buffer = this.buffer.subarray(0, newByteLength)
    this.originalByteLength = newByteLength
    this.max = newLength
    this.ptr = 0
    return newLength
  }

  /**
   * get a signed h.264-style variable bit number
   * in exponential Golomb format
   * @returns {number} (without negative zeros)
   */
  se_v() {
    const codeword = this.ue_v()
    const result = codeword & 0x01 ? 1 + (codeword >> 1) : -(codeword >> 1)
    return result === 0 ? 0 : result
  }

  /**
   * Put an exponential-Golomb coded signed integer into the bitstream
   * https://en.wikipedia.org/wiki/Exponential-Golomb_coding#Extension_to_negative_numbers
   * @param {number} val to insert
   * @returns {number} count of bits inserted
   */
  put_se_v(val: number) {
    const cw = val <= 0 ? -val << 1 : (val << 1) - 1
    return this.put_ue_v(cw)
  }
}

/**
 * Tools for handling h264 bitstream issues.
 */
export class Bitstream extends RawBitstream {
  deemulated: boolean = false
  /**
   * Construct a bitstream
   * @param stream  Buffer containing the stream, or length in bits
   */
  constructor(stream: Uint8Array | number) {
    super(stream)
    if (typeof stream !== 'number' && typeof stream !== 'undefined') {
      this.deemulated = this.hasEmulationPrevention(this.buffer)
      this.buffer = this.deemulated ? this.deemulate(this.buffer) : this.buffer
      this.max = this.buffer.byteLength << 3
    }
  }

  get stream() {
    return this.deemulated ? this.reemulate(this.buffer) : this.buffer
  }

  copyBits(from: Bitstream, ptr: number, count: number, to: number) {
    this.deemulated = from.deemulated
    super.copyBits(from, ptr, count, to)
  }

  /**
   * add emulation prevention bytes
   * @param {Uint8Array} buf
   * @returns {Uint8Array}
   */
  reemulate(buf: Uint8Array) {
    const size = Math.floor(this.originalByteLength * 1.2)
    const stream = new Uint8Array(size)
    const len = buf.byteLength - 1
    let q = 0
    let p = 0
    stream[p++] = buf[q++]!
    stream[p++] = buf[q++]!
    while (q < len) {
      if (buf[q - 2] === 0 && buf[q - 1] === 0 && buf[q]! <= 3) {
        stream[p++] = 3
        stream[p++] = buf[q++]!
      }
      stream[p++]! = buf[q++]!
    }
    stream[p++] = buf[q++]!
    return stream.subarray(0, p)
  }

  hasEmulationPrevention(stream: Uint8Array) {
    /* maybe no need to remove emulation protection? scan for 00 00 */
    for (let i = 1; i < stream.byteLength; i++) {
      if (stream[i - 1] === 0 && stream[i] === 0) {
        return true
      }
    }
    return false
  }

  /**
   * remove the emulation prevention bytes
   * @param {Uint8Array} stream
   * @returns {Uint8Array}
   */
  deemulate(stream: Uint8Array) {
    const buf = new Uint8Array(stream.byteLength)
    let p = 0
    let q = 0
    const len = stream.byteLength - 1
    buf[q++] = stream[p++]!
    buf[q++] = stream[p++]!
    /* remove emulation prevention:  00 00 03 00  means 00 00 00, 00 00 03 01 means 00 00 01 */
    while (p < len) {
      if (stream[p - 2] === 0 && stream[p - 1] === 0 && stream[p] === 3 && stream[p]! <= 3) p++
      else buf[q++] = stream[p++]!
    }
    buf[q++] = stream[p++]!
    return buf.subarray(0, q)
  }
}

export type StreamType = 'packet' | 'annexB' | 'unknown'

type NextPackageResult = { n: number; s: number; e: number; message?: string }

export class NALUStream {
  strict: boolean
  type: StreamType | null
  buf: Uint8Array
  boxSize: number | null
  cursor: number
  nextPacket: ((buf: Uint8Array, p: number, boxSize: number) => NextPackageResult) | undefined

  /**
   * Construct a NALUStream from a buffer, figuring out what kind of stream it
   * is when the options are omitted.
   * @param {Uint8Array} buf buffer with a sequence of one or more NALUs
   * @param options Pareser options
   * @param {boolean} options.strict "Throw additional exceptions"
   * @param {number} options.boxSize box size
   * @param {number} options.boxSizeMinusOne
   * @param {"packet"|"annexB"|"unknown"} options.type type
   *
   * { strict: boolean, boxSize: number, boxSizeMinusOne: number , type='packet' |'annexB'},
   */
  constructor(
    buf: Uint8Array,
    options: {
      strict?: boolean
      boxSize?: number
      boxSizeMinusOne?: number
      type: StreamType
    }
  ) {
    this.strict = false
    this.type = null
    //   this.buf = null;
    this.boxSize = null
    this.cursor = 0
    this.nextPacket = undefined

    if (options) {
      if (typeof options.strict === 'boolean') this.strict = Boolean(options.strict)
      if (options.boxSizeMinusOne) this.boxSize = options.boxSizeMinusOne + 1
      if (options.boxSize) this.boxSize = options.boxSize
      if (options.type) this.type = options.type
      if (this.type && !validNaluTypes.has(this.type))
        throw new Error('NALUStream error: type must be packet or annexB')
    }

    if (this.strict && this.boxSize && (this.boxSize < 2 || this.boxSize > 6))
      throw new Error('NALUStream error: invalid boxSize')

    /* don't copy this.buf from input, just project it */
    this.buf = new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength)

    if (!this.type || !this.boxSize) {
      const { type, boxSize } = this.getType(4)
      this.type = type
      this.boxSize = boxSize
    }
    this.nextPacket = this.type === 'packet' ? this.nextLengthCountedPacket : this.nextAnnexBPacket
  }

  get boxSizeMinusOne() {
    return this.boxSize! - 1
  }

  /**
   * getter for number of NALUs in the stream
   * @returns {number}
   */
  get packetCount() {
    return this.iterate()
  }

  /**
   * Returns an array of NALUs
   * NOTE WELL: this yields subarrays of the NALUs in the stream, not copies.
   * so changing the NALU contents also changes the stream. Beware.
   * @returns {[]}
   */
  get packets() {
    const pkts: Uint8Array[] = []
    this.iterate((buf, first, last) => {
      const pkt = buf.subarray(first, last)
      pkts.push(pkt)
    })
    return pkts
  }

  /**
   * read an n-byte unsigned number
   * @param buff
   * @param ptr
   * @param boxSize
   * @returns {number}
   */
  static readUIntNBE(buff: Uint8Array, ptr: number, boxSize: number) {
    if (!boxSize) throw new Error('readUIntNBE error: need a boxsize')
    let result = 0 | 0
    for (let i = ptr; i < ptr + boxSize; i++) {
      result = (result << 8) | buff[i]!
    }
    return result
  }

  static array2hex(array: Uint8Array) {
    // buffer is an ArrayBuffer
    return Array.prototype.map
      .call(new Uint8Array(array.buffer, array.byteOffset, array.byteLength), (x) =>
        ('00' + x.toString(16)).slice(-2)
      )
      .join(' ')
  }

  /**
   * Iterator allowing
   *      for (const nalu of stream) { }
   * Yields, space-efficiently, the elements of the stream
   * NOTE WELL: this yields subarrays of the NALUs in the stream, not copies.
   * so changing the NALU contents also changes the stream. Beware.
   * @returns {{next: next}}
   */
  [Symbol.iterator]() {
    let delim = { n: 0, s: 0, e: 0 }
    return {
      next: () => {
        if (this.type === 'unknown' || this.boxSize! < 1 || delim.n < 0)
          return { value: undefined, done: true }
        delim = this.nextPacket?.(this.buf, delim.n, this.boxSize!) ?? {
          n: 0,
          s: 0,
          e: 0
        }
        while (true) {
          if (delim.e > delim.s) {
            const pkt = this.buf.subarray(delim.s, delim.e)
            return { value: pkt, done: false }
          }
          if (delim.n < 0) break
          delim = this.nextPacket?.(this.buf, delim.n, this.boxSize!) ?? {
            n: 0,
            s: 0,
            e: 0
          }
        }
        return { value: undefined, done: true }
      }
    }
  }

  /**
   * Iterator allowing
   *      for (const n of stream.nalus()) {
   *        const {rawNalu, nalu} = n
   *       }
   * Yields, space-efficiently, the elements of the stream
   * NOTE WELL: this yields subarrays of the NALUs in the stream, not copies.
   * so changing the NALU contents also changes the stream. Beware.

   */
  nalus() {
    return {
      [Symbol.iterator]: () => {
        let delim = { n: 0, s: 0, e: 0 }
        return {
          next: () => {
            if (this.type === 'unknown' || this.boxSize! < 1 || delim.n < 0)
              return { value: undefined, done: true }
            delim = this.nextPacket?.(this.buf, delim.n, this.boxSize!) ?? {
              n: 0,
              s: 0,
              e: 0
            }
            while (true) {
              if (delim.e > delim.s) {
                const nalu = this.buf.subarray(delim.s, delim.e)
                const rawNalu = this.buf.subarray(delim.s - this.boxSize!, delim.e)
                return { value: { nalu, rawNalu }, done: false }
              }
              if (delim.n < 0) break
              delim = this.nextPacket?.(this.buf, delim.n, this.boxSize!) ?? {
                n: 0,
                s: 0,
                e: 0
              }
            }
            return { value: undefined, done: true }
          }
        }
      }
    }
  }

  /**
   * Convert an annexB stream to a packet stream in place, overwriting the buffer
   * @returns {NALUStream}
   */
  convertToPacket() {
    if (this.type === 'packet') return this
    /* change 00 00 00 01 delimiters to packet lengths */
    if (this.type === 'annexB' && this.boxSize === 4) {
      this.iterate((buff, first, last) => {
        let p = first - 4
        if (p < 0) throw new Error('NALUStream error: Unexpected packet format')
        const len = last - first
        buff[p++] = 0xff & (len >> 24)
        buff[p++] = 0xff & (len >> 16)
        buff[p++] = 0xff & (len >> 8)
        buff[p++] = 0xff & len
      })
    } else if (this.type === 'annexB' && this.boxSize === 3) {
      /* change 00 00 01 delimiters to packet lengths */
      this.iterate((buff, first, last) => {
        let p = first - 3
        if (p < 0) throw new Error('Unexpected packet format')
        const len = last - first
        if (this.strict && 0xff && len >> 24 !== 0)
          throw new Error(
            'NALUStream error: Packet too long to store length when boxLenMinusOne is 2'
          )
        buff[p++] = 0xff & (len >> 16)
        buff[p++] = 0xff & (len >> 8)
        buff[p++] = 0xff & len
      })
    }
    this.type = 'packet'
    this.nextPacket = this.nextLengthCountedPacket

    return this
  }

  convertToAnnexB() {
    if (this.type === 'annexB') return this

    if (this.type === 'packet' && this.boxSize === 4) {
      this.iterate((buff, first) => {
        let p = first - 4
        if (p < 0) throw new Error('NALUStream error: Unexpected packet format')
        buff[p++] = 0xff & 0
        buff[p++] = 0xff & 0
        buff[p++] = 0xff & 0
        buff[p++] = 0xff & 1
      })
    } else if (this.type === 'packet' && this.boxSize === 3) {
      this.iterate((buff, first) => {
        let p = first - 3
        if (p < 0) throw new Error('Unexpected packet format')
        buff[p++] = 0xff & 0
        buff[p++] = 0xff & 0
        buff[p++] = 0xff & 1
      })
    }
    this.type = 'annexB'
    this.nextPacket = this.nextAnnexBPacket

    return this
  }

  iterate(callback: ((buf: Uint8Array, s: number, e: number) => void) | undefined = undefined) {
    if (this.type === 'unknown') return 0
    if (this.boxSize! < 1) return 0
    let packetCount = 0
    let delim = this.nextPacket?.(this.buf, 0, this.boxSize!) ?? {
      n: 0,
      s: 0,
      e: 0
    }
    while (true) {
      if (delim.e > delim.s) {
        packetCount++
        if (typeof callback === 'function') callback(this.buf, delim.s, delim.e)
      }
      if (delim.n < 0) break
      delim = this.nextPacket?.(this.buf, delim.n, this.boxSize!) ?? {
        n: 0,
        s: 0,
        e: 0
      }
    }
    return packetCount
  }

  /**
   * iterator helper for delimited streams either 00 00 01  or 00 00 00 01
   * @param buf
   * @param p
   * @returns iterator
   */

  nextAnnexBPacket(buf: Uint8Array, p: number, _: number) {
    const buflen = buf.byteLength
    const start = p
    if (p === buflen) return { n: -1, s: start, e: p }
    while (p < buflen) {
      if (p + 2 > buflen) return { n: -1, s: start, e: buflen }
      if (buf[p] === 0 && buf[p + 1] === 0) {
        const d = buf[p + 2]
        if (d === 1) {
          /* 00 00 01 found */
          return { n: p + 3, s: start, e: p }
        } else if (d === 0) {
          if (p + 3 > buflen) return { n: -1, s: start, e: buflen }
          const e = buf[p + 3]
          if (e === 1) {
            /* 00 00 00 01 found */
            return { n: p + 4, s: start, e: p }
          }
        }
      }
      p++
    }
    return { n: -1, s: start, e: p }
  }

  /**
   * iterator helper for length-counted data
   * @param buf
   * @param p
   * @param boxSize
   * @returns {{s: *, e: *, n: *}|{s: number, e: number, message: string, n: number}}
   */
  nextLengthCountedPacket(buf: Uint8Array, p: number, boxSize: number) {
    const buflen = buf.byteLength
    if (p < buflen) {
      const plength = NALUStream.readUIntNBE(buf, p, boxSize)
      if (plength < 2 || plength > buflen + boxSize) {
        return { n: -2, s: 0, e: 0, message: 'bad length' }
      }
      return {
        n: p + boxSize + plength,
        s: p + boxSize,
        e: p + boxSize + plength
      }
    }
    return { n: -1, s: 0, e: 0, message: 'end of buffer' }
  }

  /**
   * figure out type of data stream
   * @returns {{boxSize: number, type: string}}
   */
  getType = (scanLimit: number): { boxSize: number; type: StreamType } => {
    if (this.type && this.boxSize) return { type: this.type, boxSize: this.boxSize }
    /* start with a delimiter? */
    if (!this.type || this.type === 'annexB') {
      if (this.buf[0] === 0 && this.buf[1] === 0 && this.buf[2] === 1) {
        return { type: 'annexB', boxSize: 3 }
      } else if (this.buf[0] === 0 && this.buf[1] === 0 && this.buf[2] === 0 && this.buf[3] === 1) {
        return { type: 'annexB', boxSize: 4 }
      }
    }
    /* possibly packet stream with lengths */
    /* try various boxSize values */
    for (let boxSize = 4; boxSize >= 1; boxSize--) {
      let packetCount = 0
      if (this.buf.length <= boxSize) {
        packetCount = -1
        break
      }
      let delim = this.nextLengthCountedPacket(this.buf, 0, boxSize)
      while (true) {
        if (delim.n < -1) {
          packetCount = -1
          break
        }
        if (delim.e - delim.s) {
          packetCount++
          if (scanLimit && packetCount >= scanLimit) break
        }
        if (delim.n < 0) break
        delim = this.nextLengthCountedPacket(this.buf, delim.n, boxSize)
      }
      if (packetCount > 0) {
        return { type: 'packet', boxSize: boxSize }
      }
    }
    if (this.strict) throw new Error('NALUStream error: cannot determine stream type or box size')
    return { type: 'unknown', boxSize: -1 }
  }
}

export class SPS {
  bitstream: Bitstream
  // private buffer: Uint8Array;
  nal_ref_id: number
  nal_unit_type: number | undefined
  profile_idc: number
  profileName: string
  constraint_set0_flag: number
  constraint_set1_flag: number
  constraint_set2_flag: number
  constraint_set3_flag: number
  constraint_set4_flag: number
  constraint_set5_flag: number
  level_idc: number
  seq_parameter_set_id: number
  has_no_chroma_format_idc: boolean
  chroma_format_idc: number | undefined
  bit_depth_luma_minus8: number | undefined
  separate_colour_plane_flag: number | undefined
  chromaArrayType: number | undefined
  bitDepthLuma: number | undefined
  bit_depth_chroma_minus8: number | undefined
  lossless_qpprime_flag: number | undefined
  bitDepthChroma: number | undefined
  seq_scaling_matrix_present_flag: number | undefined
  seq_scaling_list_present_flag: Array<number> | undefined
  seq_scaling_list: Array<number[]> | undefined
  log2_max_frame_num_minus4: number | undefined
  maxFrameNum: number
  pic_order_cnt_type: number
  log2_max_pic_order_cnt_lsb_minus4: number | undefined
  maxPicOrderCntLsb: number | undefined
  delta_pic_order_always_zero_flag: number | undefined
  offset_for_non_ref_pic: number | undefined
  offset_for_top_to_bottom_field: number | undefined
  num_ref_frames_in_pic_order_cnt_cycle: number | undefined
  offset_for_ref_frame: Array<number> | undefined
  max_num_ref_frames: number
  gaps_in_frame_num_value_allowed_flag: number
  pic_width_in_mbs_minus_1: number
  picWidth: number
  pic_height_in_map_units_minus_1: number
  frame_mbs_only_flag: number
  interlaced: boolean
  mb_adaptive_frame_field_flag: number | undefined
  picHeight: number
  direct_8x8_inference_flag: number
  frame_cropping_flag: number
  frame_cropping_rect_left_offset: number | undefined
  frame_cropping_rect_right_offset: number | undefined
  frame_cropping_rect_top_offset: number | undefined
  frame_cropping_rect_bottom_offset: number | undefined
  cropRect: { x: number; y: number; width: number; height: number }
  vui_parameters_present_flag: number
  aspect_ratio_info_present_flag: number | undefined
  aspect_ratio_idc: number | undefined
  sar_width: number | undefined
  sar_height: number | undefined
  overscan_info_present_flag: number | undefined
  overscan_appropriate_flag: number | undefined
  video_signal_type_present_flag: number | undefined
  video_format: number | undefined
  video_full_range_flag: number | undefined
  color_description_present_flag: number | undefined
  color_primaries: number | undefined
  transfer_characteristics: number | undefined
  matrix_coefficients: number | undefined
  chroma_loc_info_present_flag: number | undefined
  chroma_sample_loc_type_top_field: number | undefined
  chroma_sample_loc_type_bottom_field: number | undefined
  timing_info_present_flag: number | undefined
  num_units_in_tick: number | undefined
  time_scale: number | undefined
  fixed_frame_rate_flag: number | undefined
  framesPerSecond: number | undefined
  nal_hrd_parameters_present_flag: number | undefined

  success: boolean

  constructor(SPS: Uint8Array) {
    const bitstream = new Bitstream(SPS)
    this.bitstream = bitstream
    // this.buffer = bitstream.buffer

    const forbidden_zero_bit = bitstream.u_1()
    if (forbidden_zero_bit) throw new Error('NALU error: invalid NALU header')
    this.nal_ref_id = bitstream.u_2()
    this.nal_unit_type = bitstream.u(5)
    if (this.nal_unit_type !== 7) throw new Error('SPS error: not SPS')

    this.profile_idc = bitstream.u_8()!
    if (profileNames.has(this.profile_idc)) {
      this.profileName = profileNames.get(this.profile_idc)!
    } else {
      throw new Error('SPS error: invalid profile_idc')
    }

    this.constraint_set0_flag = bitstream.u_1()
    this.constraint_set1_flag = bitstream.u_1()
    this.constraint_set2_flag = bitstream.u_1()
    this.constraint_set3_flag = bitstream.u_1()
    this.constraint_set4_flag = bitstream.u_1()
    this.constraint_set5_flag = bitstream.u_1()
    const reserved_zero_2bits = bitstream.u_2()
    if (reserved_zero_2bits !== 0) throw new Error('SPS error: reserved_zero_2bits must be zero')

    this.level_idc = bitstream.u_8()!

    this.seq_parameter_set_id = bitstream.ue_v()
    if (this.seq_parameter_set_id > 31)
      throw new Error('SPS error: seq_parameter_set_id must be 31 or less')

    this.has_no_chroma_format_idc =
      this.profile_idc === 66 || this.profile_idc === 77 || this.profile_idc === 88

    if (!this.has_no_chroma_format_idc) {
      this.chroma_format_idc = bitstream.ue_v()
      if (this.bit_depth_luma_minus8 && this.bit_depth_luma_minus8 > 3)
        throw new Error('SPS error: chroma_format_idc must be 3 or less')
      if (this.chroma_format_idc === 3) {
        /* 3 = YUV444 */
        this.separate_colour_plane_flag = bitstream.u_1()
        this.chromaArrayType = this.separate_colour_plane_flag ? 0 : this.chroma_format_idc
      }
      this.bit_depth_luma_minus8 = bitstream.ue_v()
      if (this.bit_depth_luma_minus8 > 6)
        throw new Error('SPS error: bit_depth_luma_minus8 must be 6 or less')
      this.bitDepthLuma = this.bit_depth_luma_minus8 + 8
      this.bit_depth_chroma_minus8 = bitstream.ue_v()
      if (this.bit_depth_chroma_minus8 > 6)
        throw new Error('SPS error: bit_depth_chroma_minus8 must be 6 or less')
      this.lossless_qpprime_flag = bitstream.u_1()
      this.bitDepthChroma = this.bit_depth_chroma_minus8 + 8
      this.seq_scaling_matrix_present_flag = bitstream.u_1()
      if (this.seq_scaling_matrix_present_flag) {
        const n_ScalingList = this.chroma_format_idc !== 3 ? 8 : 12
        this.seq_scaling_list_present_flag = []
        this.seq_scaling_list = []
        for (let i = 0; i < n_ScalingList; i++) {
          const seqScalingListPresentFlag = bitstream.u_1()
          this.seq_scaling_list_present_flag.push(seqScalingListPresentFlag)
          if (seqScalingListPresentFlag) {
            const sizeOfScalingList = i < 6 ? 16 : 64
            let nextScale = 8
            let lastScale = 8
            const delta_scale: number[] = []
            for (let j = 0; j < sizeOfScalingList; j++) {
              if (nextScale !== 0) {
                const deltaScale = bitstream.se_v()
                delta_scale.push(deltaScale)
                nextScale = (lastScale + deltaScale + 256) % 256
              }
              lastScale = nextScale === 0 ? lastScale : nextScale
              this.seq_scaling_list.push(delta_scale)
            }
          }
        }
      }
    }

    this.log2_max_frame_num_minus4 = bitstream.ue_v()
    if (this.log2_max_frame_num_minus4 > 12)
      throw new Error('SPS error: log2_max_frame_num_minus4 must be 12 or less')
    this.maxFrameNum = 1 << (this.log2_max_frame_num_minus4 + 4)

    this.pic_order_cnt_type = bitstream.ue_v()
    if (this.pic_order_cnt_type > 2)
      throw new Error('SPS error: pic_order_cnt_type must be 2 or less')

    // let expectedDeltaPerPicOrderCntCycle = 0
    switch (this.pic_order_cnt_type) {
      case 0:
        this.log2_max_pic_order_cnt_lsb_minus4 = bitstream.ue_v()
        if (this.log2_max_pic_order_cnt_lsb_minus4 > 12)
          throw new Error('SPS error: log2_max_pic_order_cnt_lsb_minus4 must be 12 or less')
        this.maxPicOrderCntLsb = 1 << (this.log2_max_pic_order_cnt_lsb_minus4 + 4)
        break
      case 1:
        this.delta_pic_order_always_zero_flag = bitstream.u_1()
        this.offset_for_non_ref_pic = bitstream.se_v()
        this.offset_for_top_to_bottom_field = bitstream.se_v()
        this.num_ref_frames_in_pic_order_cnt_cycle = bitstream.ue_v()
        this.offset_for_ref_frame = []
        for (let i = 0; i < this.num_ref_frames_in_pic_order_cnt_cycle; i++) {
          const offsetForRefFrame = bitstream.se_v()
          this.offset_for_ref_frame.push(offsetForRefFrame)

          // expectedDeltaPerPicOrderCntCycle += offsetForRefFrame
        }
        break
      case 2:
        /* there is nothing for case 2 */
        break
    }

    this.max_num_ref_frames = bitstream.ue_v()
    this.gaps_in_frame_num_value_allowed_flag = bitstream.u_1()
    this.pic_width_in_mbs_minus_1 = bitstream.ue_v()
    this.picWidth = (this.pic_width_in_mbs_minus_1 + 1) << 4
    this.pic_height_in_map_units_minus_1 = bitstream.ue_v()
    this.frame_mbs_only_flag = bitstream.u_1()
    this.interlaced = !this.frame_mbs_only_flag
    if (this.frame_mbs_only_flag === 0) {
      /* 1 if frames rather than fields (no interlacing) */
      this.mb_adaptive_frame_field_flag = bitstream.u_1()
    }
    this.picHeight =
      ((2 - this.frame_mbs_only_flag) * (this.pic_height_in_map_units_minus_1 + 1)) << 4

    this.direct_8x8_inference_flag = bitstream.u_1()
    this.frame_cropping_flag = bitstream.u_1()
    if (this.frame_cropping_flag) {
      this.frame_cropping_rect_left_offset = bitstream.ue_v()
      this.frame_cropping_rect_right_offset = bitstream.ue_v()
      this.frame_cropping_rect_top_offset = bitstream.ue_v()
      this.frame_cropping_rect_bottom_offset = bitstream.ue_v()
      this.cropRect = {
        x: this.frame_cropping_rect_left_offset,
        y: this.frame_cropping_rect_top_offset,
        width:
          this.picWidth -
          (this.frame_cropping_rect_left_offset + this.frame_cropping_rect_right_offset),
        height:
          this.picHeight -
          (this.frame_cropping_rect_top_offset + this.frame_cropping_rect_bottom_offset)
      }
    } else {
      this.cropRect = {
        x: 0,
        y: 0,
        width: this.picWidth,
        height: this.picHeight
      }
    }
    this.vui_parameters_present_flag = bitstream.u_1()
    if (this.vui_parameters_present_flag) {
      this.aspect_ratio_info_present_flag = bitstream.u_1()
      if (this.aspect_ratio_info_present_flag) {
        this.aspect_ratio_idc = bitstream.u_8()
        if (this.aspect_ratio_idc) {
          this.sar_width = bitstream.u(16)
          this.sar_height = bitstream.u(16)
        }
      }

      this.overscan_info_present_flag = bitstream.u_1()
      if (this.overscan_info_present_flag) this.overscan_appropriate_flag = bitstream.u_1()
      this.video_signal_type_present_flag = bitstream.u_1()
      if (this.video_signal_type_present_flag) {
        this.video_format = bitstream.u(3)
        this.video_full_range_flag = bitstream.u_1()
        this.color_description_present_flag = bitstream.u_1()
        if (this.color_description_present_flag) {
          this.color_primaries = bitstream.u_8()
          this.transfer_characteristics = bitstream.u_8()
          this.matrix_coefficients = bitstream.u_8()
        }
      }
      this.chroma_loc_info_present_flag = bitstream.u_1()
      if (this.chroma_loc_info_present_flag) {
        this.chroma_sample_loc_type_top_field = bitstream.ue_v()
        this.chroma_sample_loc_type_bottom_field = bitstream.ue_v()
      }
      this.timing_info_present_flag = bitstream.u_1()
      if (this.timing_info_present_flag) {
        this.num_units_in_tick = bitstream.u(32)
        this.time_scale = bitstream.u(32)
        this.fixed_frame_rate_flag = bitstream.u_1()
        if (this.num_units_in_tick && this.time_scale && this.num_units_in_tick > 0) {
          this.framesPerSecond = this.time_scale / (2 * this.num_units_in_tick)
        }
      }
      this.nal_hrd_parameters_present_flag = bitstream.u_1()
    }
    this.success = true
  }

  get stream() {
    return this.bitstream.stream
  }

  get profile_compatibility() {
    let v = this.constraint_set0_flag << 7
    v |= this.constraint_set1_flag << 6
    v |= this.constraint_set2_flag << 5
    v |= this.constraint_set3_flag << 4
    v |= this.constraint_set4_flag << 3
    v |= this.constraint_set5_flag << 1
    return v
  }

  /**
   * getter for the MIME type encoded in this avcC
   * @returns {string}
   */
  get MIME() {
    const f: string[] = []
    f.push('avc1.')
    f.push(byte2hex(this.profile_idc).toUpperCase())
    f.push(byte2hex(this.profile_compatibility).toUpperCase())
    f.push(byte2hex(this.level_idc).toUpperCase())
    return f.join('')
  }
}
