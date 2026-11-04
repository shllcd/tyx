const MAX_VARINT_BYTES = 10;

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

export class Writer {
  private buf: Uint8Array;
  private len = 0;

  constructor(capacity = 256) {
    this.buf = new Uint8Array(capacity);
  }

  get length(): number {
    return this.len;
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

  varint(value: number | bigint): void {
    if (typeof value === "number") {
      if (!Number.isSafeInteger(value) || value < 0) throw new TyxError(`varint out of range: ${value}`);
      while (value >= 0x80) {
        this.byte((value % 0x80) | 0x80);
        value = Math.floor(value / 0x80);
      }
      this.byte(value);
      return;
    }
    if (value < 0n || value > 0xffff_ffff_ffff_ffffn) throw new TyxError(`varint out of range: ${value}`);
    while (value >= 0x80n) {
      this.byte(Number(value & 0x7fn) | 0x80);
      value >>= 7n;
    }
    this.byte(Number(value));
  }

  zigzag(value: bigint): void {
    this.varint((value << 1n) ^ (value >> 63n));
  }

  lenPrefixed(bytes: Uint8Array): void {
    this.varint(bytes.length);
    this.bytes(bytes);
  }

  tag(index: number, hasPayload: boolean): void {
    this.varint(index * 2 + (hasPayload ? 1 : 0));
  }

  begin(mask: bigint): number {
    const start = this.len;
    this.byte(mask === 0n ? 0 : 1);
    if (mask !== 0n) this.varint(mask);
    return start;
  }

  end(start: number): void {
    const len = this.len - start - 1;
    const header = BigInt(len) * 2n + BigInt(this.buf[start]!);
    if (header < 0x80n) {
      this.buf[start] = Number(header);
      return;
    }
    const encoded: number[] = [];
    let rest = header;
    while (rest >= 0x80n) {
      encoded.push(Number(rest & 0x7fn) | 0x80);
      rest >>= 7n;
    }
    encoded.push(Number(rest));
    this.reserve(encoded.length);
    this.buf.copyWithin(start + encoded.length, start + 1, this.len);
    this.buf.set(encoded, start);
    this.len += encoded.length - 1;
  }

  private reserve(extra: number): void {
    const needed = this.len + extra;
    if (needed <= this.buf.length) return;
    let capacity = this.buf.length * 2 || 1;
    while (capacity < needed) capacity *= 2;
    const next = new Uint8Array(capacity);
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
  }
}

export class Reader {
  private readonly buf: Uint8Array;
  private pos = 0;

  constructor(bytes: Uint8Array) {
    this.buf = bytes;
  }

  get remaining(): number {
    return this.buf.length - this.pos;
  }

  get isEmpty(): boolean {
    return this.pos === this.buf.length;
  }

  byte(): number {
    if (this.pos >= this.buf.length) throw new TyxError("unexpected end of input");
    return this.buf[this.pos++]!;
  }

  take(len: number): Uint8Array {
    if (len > this.remaining) throw new TyxError("unexpected end of input");
    const bytes = this.buf.subarray(this.pos, this.pos + len);
    this.pos += len;
    return bytes;
  }

  varint(): bigint {
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

  zigzag(): bigint {
    const raw = this.varint();
    return (raw >> 1n) ^ -(raw & 1n);
  }

  length(): number {
    const value = this.varint();
    if (value > BigInt(this.remaining)) throw new TyxError("unexpected end of input");
    return Number(value);
  }

  lenPrefixed(): Uint8Array {
    return this.take(this.length());
  }

  tag(): [index: number, hasPayload: boolean] {
    const raw = this.varint();
    const index = raw >> 1n;
    if (index > 0xffff_ffffn) throw new TyxError("varint does not fit its type");
    return [Number(index), (raw & 1n) === 1n];
  }

  frame(): [body: Reader, mask: bigint] {
    const header = this.varint();
    const len = header >> 1n;
    if (len > BigInt(this.remaining)) throw new TyxError("unexpected end of input");
    const body = new Reader(this.take(Number(len)));
    const mask = (header & 1n) === 1n ? body.varint() : 0n;
    return [body, mask];
  }

  skipFrame(): void {
    const header = this.varint();
    const len = header >> 1n;
    if (len > BigInt(this.remaining)) throw new TyxError("unexpected end of input");
    this.pos += Number(len);
  }
}
