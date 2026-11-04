const MAX_VARINT_BYTES = 10;
const ASCII_ENCODE_FAST_PATH_MAX = 64;
const ASCII_DECODE_FAST_PATH_MAX = 16;

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });

export class TyxError extends Error {
  path: string;

  constructor(message: string, path = "") {
    super(message);
    this.name = "TyxError";
    this.path = path;
  }

  override toString(): string {
    return this.path ? `${this.name}: ${this.path}: ${this.message}` : super.toString();
  }
}

function varintSize(value: number): number {
  let size = 1;
  while (value >= 0x80) {
    value = Math.floor(value / 0x80);
    size++;
  }
  return size;
}

export class Writer {
  private buf: Uint8Array;
  private view: DataView;
  private len = 0;

  constructor(capacity = 256) {
    this.buf = new Uint8Array(capacity);
    this.view = new DataView(this.buf.buffer);
  }

  get length(): number {
    return this.len;
  }

  get capacity(): number {
    return this.buf.length;
  }

  clear(): void {
    this.len = 0;
  }

  intoBytes(): Uint8Array {
    return this.buf.slice(0, this.len);
  }

  byte(value: number): void {
    this.reserve(1);
    this.buf[this.len++] = value;
  }

  bytes(values: Uint8Array): void {
    this.reserve(values.length);
    this.buf.set(values, this.len);
    this.len += values.length;
  }

  f32(value: number): void {
    this.reserve(4);
    this.view.setFloat32(this.len, value, true);
    this.len += 4;
  }

  f64(value: number): void {
    this.reserve(8);
    this.view.setFloat64(this.len, value, true);
    this.len += 8;
  }

  varint(value: number | bigint): void {
    if (typeof value === "number") {
      if (!Number.isSafeInteger(value) || value < 0) throw new TyxError(`varint out of range: ${value}`);
      this.uvarint(value);
      return;
    }
    if (value < 0n || value > 0xffff_ffff_ffff_ffffn) throw new TyxError(`varint out of range: ${value}`);
    if (value <= BigInt(Number.MAX_SAFE_INTEGER)) {
      this.uvarint(Number(value));
      return;
    }
    while (value >= 0x80n) {
      this.byte(Number(value & 0x7fn) | 0x80);
      value >>= 7n;
    }
    this.byte(Number(value));
  }

  /** Writes `value` unchecked; it must be a safe non-negative integer. */
  uvarint(value: number): void {
    this.reserve(MAX_VARINT_BYTES);
    this.len = this.put(this.len, value);
  }

  zigzag(value: bigint): void {
    this.varint((value << 1n) ^ (value >> 63n));
  }

  /** Zigzag-encodes an integer `value`; magnitudes of 2^51 and above take the `bigint` path. */
  zigzagNumber(value: number): void {
    if (value >= -0x8_0000_0000_0000 && value < 0x8_0000_0000_0000) {
      this.uvarint(value >= 0 ? value * 2 : -value * 2 - 1);
      return;
    }
    this.zigzag(BigInt(value));
  }

  lenPrefixed(bytes: Uint8Array): void {
    this.varint(bytes.length);
    this.bytes(bytes);
  }

  /** Writes `value` length-prefixed as UTF-8; lone surrogates become U+FFFD. */
  str(value: string): void {
    const length = value.length;
    if (length <= ASCII_ENCODE_FAST_PATH_MAX) {
      this.reserve(1 + length);
      const buf = this.buf;
      const start = this.len + 1;
      let index = 0;
      while (index < length) {
        const code = value.charCodeAt(index);
        if (code >= 0x80) break;
        buf[start + index++] = code;
      }
      if (index === length) {
        buf[this.len] = length;
        this.len = start + length;
        return;
      }
    }
    const maxBytes = length * 3;
    this.reserve(MAX_VARINT_BYTES + maxBytes);
    const reservedPrefix = varintSize(maxBytes);
    const { written } = encoder.encodeInto(value, this.buf.subarray(this.len + reservedPrefix));
    const prefix = varintSize(written);
    if (prefix !== reservedPrefix) {
      this.buf.copyWithin(this.len + prefix, this.len + reservedPrefix, this.len + reservedPrefix + written);
    }
    this.uvarint(written);
    this.len += written;
  }

  tag(index: number, hasPayload: boolean): void {
    this.varint(index * 2 + (hasPayload ? 1 : 0));
  }

  begin(mask: number | bigint): number {
    const start = this.len;
    const hasMask = mask !== 0 && mask !== 0n;
    this.byte(hasMask ? 1 : 0);
    if (hasMask) this.varint(mask);
    return start;
  }

  end(start: number): void {
    const header = (this.len - start - 1) * 2 + this.buf[start]!;
    if (header < 0x80) {
      this.buf[start] = header;
      return;
    }
    const extra = varintSize(header) - 1;
    this.reserve(extra);
    this.buf.copyWithin(start + 1 + extra, start + 1, this.len);
    this.put(start, header);
    this.len += extra;
  }

  private put(at: number, value: number): number {
    const buf = this.buf;
    while (value >= 0x80) {
      buf[at++] = (value & 0x7f) | 0x80;
      value = value > 0xffff_ffff ? Math.floor(value / 0x80) : value >>> 7;
    }
    buf[at++] = value;
    return at;
  }

  private reserve(extra: number): void {
    const needed = this.len + extra;
    if (needed <= this.buf.length) return;
    let capacity = this.buf.length * 2 || 1;
    while (capacity < needed) capacity *= 2;
    const next = new Uint8Array(capacity);
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
    this.view = new DataView(next.buffer);
  }
}

export class Reader {
  private readonly buf: Uint8Array;
  private view: DataView | undefined;
  private pos = 0;
  private end: number;
  private readonly outerEnds: number[] = [];

  constructor(bytes: Uint8Array) {
    this.buf = bytes;
    this.end = bytes.length;
  }

  get remaining(): number {
    return this.end - this.pos;
  }

  get isEmpty(): boolean {
    return this.pos === this.end;
  }

  byte(): number {
    if (this.pos >= this.end) throw new TyxError("unexpected end of input");
    return this.buf[this.pos++]!;
  }

  take(len: number): Uint8Array {
    if (len > this.remaining) throw new TyxError("unexpected end of input");
    const bytes = this.buf.subarray(this.pos, this.pos + len);
    this.pos += len;
    return bytes;
  }

  f32(): number {
    if (4 > this.remaining) throw new TyxError("unexpected end of input");
    const value = this.dataView().getFloat32(this.pos, true);
    this.pos += 4;
    return value;
  }

  f64(): number {
    if (8 > this.remaining) throw new TyxError("unexpected end of input");
    const value = this.dataView().getFloat64(this.pos, true);
    this.pos += 8;
    return value;
  }

  varint(): bigint {
    const start = this.pos;
    const value = this.varintNumber();
    if (value <= Number.MAX_SAFE_INTEGER) return BigInt(value);
    this.pos = start;
    return this.varintBig();
  }

  /** Reads a varint of at most 64 bits as a `number`, exact up to `Number.MAX_SAFE_INTEGER`. */
  varintNumber(): number {
    const pos = this.pos;
    if (pos < this.end) {
      const first = this.buf[pos]!;
      if (first < 0x80) {
        this.pos = pos + 1;
        return first;
      }
    }
    let value = 0;
    let scale = 1;
    for (let index = 0; index < MAX_VARINT_BYTES; index++) {
      const byte = this.byte();
      const bits = byte & 0x7f;
      if (index === MAX_VARINT_BYTES - 1 && bits > 1) throw new TyxError("varint does not fit its type");
      value += bits * scale;
      if (byte < 0x80) return value;
      scale *= 0x80;
    }
    throw new TyxError("varint does not fit its type");
  }

  zigzag(): bigint {
    const raw = this.varint();
    return (raw >> 1n) ^ -(raw & 1n);
  }

  /** Reads a zigzag varint as a `number`, exact for magnitudes below 2^52. */
  zigzagNumber(): number {
    const raw = this.varintNumber();
    return raw % 2 === 1 ? -(raw + 1) / 2 : raw / 2;
  }

  length(): number {
    const value = this.varintNumber();
    if (value > this.remaining) throw new TyxError("unexpected end of input");
    return value;
  }

  lenPrefixed(): Uint8Array {
    return this.take(this.length());
  }

  str(): string {
    const len = this.length();
    const start = this.pos;
    if (len <= ASCII_DECODE_FAST_PATH_MAX) {
      const buf = this.buf;
      let text = "";
      let index = start;
      const stop = start + len;
      while (index < stop) {
        const byte = buf[index]!;
        if (byte >= 0x80) break;
        text += String.fromCharCode(byte);
        index++;
      }
      if (index === stop) {
        this.pos = stop;
        return text;
      }
    }
    const bytes = this.take(len);
    try {
      return decoder.decode(bytes);
    } catch {
      throw new TyxError("invalid utf-8 in string");
    }
  }

  tag(): [index: number, hasPayload: boolean] {
    const raw = this.tagRaw();
    return [Math.floor(raw / 2), raw % 2 === 1];
  }

  /** Reads an enum tag as `index * 2 + hasPayload`. */
  tagRaw(): number {
    const raw = this.varintNumber();
    if (raw >= 0x2_0000_0000) throw new TyxError("varint does not fit its type");
    return raw;
  }

  frame(): [body: Reader, mask: bigint] {
    const header = this.varintNumber();
    const len = Math.floor(header / 2);
    if (len > this.remaining) throw new TyxError("unexpected end of input");
    const body = new Reader(this.take(len));
    const mask = header % 2 === 1 ? body.varint() : 0n;
    return [body, mask];
  }

  /**
   * Enters the next frame and returns its mask, a `bigint` only when it exceeds `Number.MAX_SAFE_INTEGER`.
   * Reads stop at the frame's end until `leaveFrame` jumps past it.
   */
  enterFrame(): number | bigint {
    const header = this.varintNumber();
    const len = Math.floor(header / 2);
    if (len > this.remaining) throw new TyxError("unexpected end of input");
    this.outerEnds.push(this.end);
    this.end = this.pos + len;
    if (header % 2 === 0) return 0;
    const start = this.pos;
    const mask = this.varintNumber();
    if (mask <= Number.MAX_SAFE_INTEGER) return mask;
    this.pos = start;
    return this.varintBig();
  }

  leaveFrame(): void {
    this.pos = this.end;
    this.end = this.outerEnds.pop()!;
  }

  skipFrame(): void {
    const header = this.varintNumber();
    const len = Math.floor(header / 2);
    if (len > this.remaining) throw new TyxError("unexpected end of input");
    this.pos += len;
  }

  private dataView(): DataView {
    return (this.view ??= new DataView(this.buf.buffer, this.buf.byteOffset, this.buf.byteLength));
  }

  private varintBig(): bigint {
    let value = 0n;
    for (let shift = 0n; shift < BigInt(MAX_VARINT_BYTES) * 7n; shift += 7n) {
      const byte = this.byte();
      const bits = BigInt(byte & 0x7f);
      if (shift === 63n && bits > 1n) throw new TyxError("varint does not fit its type");
      value |= bits << shift;
      if (byte < 0x80) return value;
    }
    throw new TyxError("varint does not fit its type");
  }
}
