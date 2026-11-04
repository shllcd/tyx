import type { Def, Fields, Schema, Ty, Variant } from "./ir.js";
import { Reader, TyxError, Writer } from "./wire.js";

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
const scratch = new DataView(new ArrayBuffer(8));
const scratchBytes = new Uint8Array(scratch.buffer);

const I64_MIN = -0x8000_0000_0000_0000n;
const I64_MAX = 0x7fff_ffff_ffff_ffffn;
const U64_MAX = 0xffff_ffff_ffff_ffffn;

/**
 * Encodes and decodes values through an IR schema. `u64` and `i64` are `bigint`; every other
 * integer is `number`. See the README for the full value mapping.
 */
export class Codec<Types extends object = Record<string, unknown>> {
  private readonly types = new Map<string, Def>();

  constructor(schema: Schema) {
    for (const def of schema.types) {
      if (this.types.has(def.name)) throw new TyxError(`type \`${def.name}\` is defined twice`);
      this.types.set(def.name, def);
    }
  }

  encode<K extends Extract<keyof Types, string>>(name: K, value: Types[K]): Uint8Array;
  encode(ty: Ty, value: unknown): Uint8Array;
  encode(ty: Ty | string, value: unknown): Uint8Array {
    const w = new Writer();
    new Cursor(this.types).write(w, named(ty), [], value);
    return w.intoBytes();
  }

  decode<K extends Extract<keyof Types, string>>(name: K, bytes: Uint8Array): Types[K];
  decode(ty: Ty, bytes: Uint8Array): unknown;
  decode(ty: Ty | string, bytes: Uint8Array): unknown {
    const r = new Reader(bytes);
    const value = new Cursor(this.types).read(r, named(ty), []);
    if (!r.isEmpty) throw new TyxError("trailing bytes after the value");
    return value;
  }
}

function named(ty: Ty | string): Ty {
  return typeof ty === "string" ? { kind: "named", name: ty } : ty;
}

function* variantIndexes(variants: readonly Variant[]): Generator<[number, Variant]> {
  let next = 0;
  for (const variant of variants) {
    const index = variant.index ?? next;
    next = (index + 1) >>> 0;
    yield [index, variant];
  }
}

function substitute(ty: Ty, env: readonly Ty[]): Ty {
  switch (ty.kind) {
    case "param":
      return env[ty.index] ?? ty;
    case "option":
      return { kind: "option", of: substitute(ty.of, env) };
    case "seq":
      return { kind: "seq", of: substitute(ty.of, env) };
    case "set":
      return { kind: "set", of: substitute(ty.of, env) };
    case "array":
      return { kind: "array", of: substitute(ty.of, env), len: ty.len };
    case "map":
      return { kind: "map", key: substitute(ty.key, env), value: substitute(ty.value, env) };
    case "tuple":
      return { kind: "tuple", of: ty.of.map((item) => substitute(item, env)) };
    case "named":
      return { kind: "named", name: ty.name, args: (ty.args ?? []).map((arg) => substitute(arg, env)) };
    default:
      return ty;
  }
}

function zeroValue(ty: Ty): unknown {
  switch (ty.kind) {
    case "bool":
      return false;
    case "u8":
    case "u16":
    case "u32":
    case "i8":
    case "i16":
    case "i32":
    case "f32":
    case "f64":
      return 0;
    case "u64":
    case "i64":
      return 0n;
    case "str":
    case "bytes":
      return "";
    case "seq":
    case "set":
      return [];
    case "map":
      return ty.key.kind === "str" ? Object.create(null) : [];
    default:
      return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isWellFormed(value: string): boolean {
  for (let index = 0; index < value.length; index++) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return false;
      index++;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return false;
    }
  }
  return true;
}

function show(value: unknown): string {
  if (typeof value === "bigint") return `${value}n`;
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

function parseHex(value: unknown): Uint8Array {
  if (typeof value !== "string" || value.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(value)) {
    throw new TyxError("expected a hex string");
  }
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < bytes.length; index++) {
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

function formatHex(bytes: Uint8Array): string {
  let text = "";
  for (const byte of bytes) text += byte.toString(16).padStart(2, "0");
  return text;
}

function parseUuid(value: unknown): Uint8Array {
  if (typeof value !== "string" || value.length !== 36) throw new TyxError("expected a hyphenated uuid");
  const digits = value.split("-").join("");
  if (digits.length !== 32) throw new TyxError("expected a hyphenated uuid");
  return parseHex(digits);
}

function formatUuid(bytes: Uint8Array): string {
  const digits = formatHex(bytes);
  return `${digits.slice(0, 8)}-${digits.slice(8, 12)}-${digits.slice(12, 16)}-${digits.slice(16, 20)}-${digits.slice(20)}`;
}

const RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/;

function parseTimestamp(value: unknown): bigint {
  if (typeof value !== "string" || !RFC3339.test(value)) throw new TyxError("expected an RFC 3339 timestamp");
  const millis = Date.parse(value);
  if (Number.isNaN(millis)) throw new TyxError("expected an RFC 3339 timestamp");
  return BigInt(millis);
}

function formatTimestamp(millis: bigint): string {
  const date = new Date(Number(millis));
  const year = date.getUTCFullYear();
  if (Number.isNaN(date.getTime()) || year < 0 || year > 9999) throw new TyxError("timestamp out of range");
  return date.toISOString().replace(/\.?0+Z$/, "Z");
}

class Cursor {
  private readonly types: Map<string, Def>;

  constructor(types: Map<string, Def>) {
    this.types = types;
  }

  private fail(message: string): TyxError {
    return new TyxError(message);
  }

  private scoped<T>(segment: string, run: () => T): T {
    try {
      return run();
    } catch (error) {
      if (error instanceof TyxError) error.path = segment + error.path;
      throw error;
    }
  }

  private def(name: string): Def {
    const def = this.types.get(name);
    if (def === undefined) throw this.fail(`unknown type \`${name}\``);
    return def;
  }

  private int(value: unknown, min: number, max: number): number {
    if (typeof value !== "number" || !Number.isInteger(value) || value < min || value > max) {
      throw this.fail(`expected an integer in ${min}..=${max}, got ${show(value)}`);
    }
    return value;
  }

  private bigint(value: unknown, min: bigint, max: bigint): bigint {
    const result =
      typeof value === "bigint" ? value : typeof value === "number" && Number.isSafeInteger(value) ? BigInt(value) : undefined;
    if (result === undefined || result < min || result > max) {
      throw this.fail(`expected an integer in ${min}..=${max}, got ${show(value)}`);
    }
    return result;
  }

  private string(value: unknown): string {
    if (typeof value !== "string") throw this.fail("expected a string");
    if (!isWellFormed(value)) throw this.fail("expected a well-formed string");
    return value;
  }

  private float(value: unknown): number {
    if (typeof value !== "number") throw this.fail("expected a number");
    return value;
  }

  write(w: Writer, ty: Ty, env: readonly Ty[], value: unknown): void {
    switch (ty.kind) {
      case "unit":
        return;
      case "bool":
        if (typeof value !== "boolean") throw this.fail("expected a boolean");
        w.byte(value ? 1 : 0);
        return;
      case "u8":
        w.byte(this.int(value, 0, 0xff));
        return;
      case "i8":
        w.byte(this.int(value, -0x80, 0x7f) & 0xff);
        return;
      case "u16":
        w.varint(this.int(value, 0, 0xffff));
        return;
      case "u32":
        w.varint(this.int(value, 0, 0xffff_ffff));
        return;
      case "u64":
        w.varint(this.bigint(value, 0n, U64_MAX));
        return;
      case "i16":
        w.zigzag(BigInt(this.int(value, -0x8000, 0x7fff)));
        return;
      case "i32":
        w.zigzag(BigInt(this.int(value, -0x8000_0000, 0x7fff_ffff)));
        return;
      case "i64":
        w.zigzag(this.bigint(value, I64_MIN, I64_MAX));
        return;
      case "f32":
        scratch.setFloat32(0, this.float(value), true);
        w.bytes(new Uint8Array(scratch.buffer, 0, 4));
        return;
      case "f64":
        scratch.setFloat64(0, this.float(value), true);
        w.bytes(new Uint8Array(scratch.buffer, 0, 8));
        return;
      case "str":
        w.lenPrefixed(encoder.encode(this.string(value)));
        return;
      case "bytes":
        w.lenPrefixed(parseHex(value));
        return;
      case "uuid":
        w.bytes(parseUuid(value));
        return;
      case "timestamp":
        w.zigzag(parseTimestamp(value));
        return;
      case "option": {
        const absent = value === null || value === undefined;
        w.byte(absent ? 0 : 1);
        if (!absent) this.write(w, ty.of, env, value);
        return;
      }
      case "seq":
      case "set": {
        if (!Array.isArray(value)) throw this.fail("expected an array");
        w.varint(value.length);
        this.writeItems(w, ty.of, env, value);
        return;
      }
      case "array": {
        if (!Array.isArray(value) || value.length !== ty.len) throw this.fail(`expected an array of ${ty.len}`);
        this.writeItems(w, ty.of, env, value);
        return;
      }
      case "tuple": {
        if (!Array.isArray(value) || value.length !== ty.of.length) throw this.fail(`expected an array of ${ty.of.length}`);
        for (const [index, item] of ty.of.entries()) {
          this.scoped(`[${index}]`, () => this.write(w, item, env, value[index]));
        }
        return;
      }
      case "map":
        this.writeMap(w, ty.key, ty.value, env, value);
        return;
      case "named": {
        const args = (ty.args ?? []).map((arg) => substitute(arg, env));
        this.writeNamed(w, ty.name, args, value);
        return;
      }
      case "param": {
        const bound = env[ty.index];
        if (bound === undefined) throw this.fail(`unbound parameter $${ty.index}`);
        this.write(w, bound, [], value);
        return;
      }
    }
  }

  private writeItems(w: Writer, ty: Ty, env: readonly Ty[], items: readonly unknown[]): void {
    for (const [index, item] of items.entries()) {
      this.scoped(`[${index}]`, () => this.write(w, ty, env, item));
    }
  }

  private writeMap(w: Writer, key: Ty, value: Ty, env: readonly Ty[], map: unknown): void {
    if (isRecord(map)) {
      if (substitute(key, env).kind !== "str") throw this.fail("only maps with string keys are objects; use [[key, value], ...]");
      const names = Object.keys(map);
      w.varint(names.length);
      for (const name of names) {
        w.lenPrefixed(encoder.encode(name));
        this.scoped(`.${name}`, () => this.write(w, value, env, map[name]));
      }
      return;
    }
    if (!Array.isArray(map)) throw this.fail("expected an object or an array of [key, value]");
    w.varint(map.length);
    for (const [index, pair] of map.entries()) {
      this.scoped(`[${index}]`, () => {
        if (!Array.isArray(pair) || pair.length !== 2) throw this.fail("expected [key, value]");
        this.write(w, key, env, pair[0]);
        this.write(w, value, env, pair[1]);
      });
    }
  }

  private writeNamed(w: Writer, name: string, args: readonly Ty[], value: unknown): void {
    const def = this.def(name);
    switch (def.kind) {
      case "newtype":
        this.write(w, def.ty, args, value);
        return;
      case "struct":
        this.writeFields(w, def.fields, args, value);
        return;
      case "enum":
        this.writeEnum(w, def, args, value);
        return;
    }
  }

  private writeEnum(w: Writer, def: Extract<Def, { kind: "enum" }>, args: readonly Ty[], value: unknown): void {
    let variantName: string;
    let hasPayload: boolean;
    let payload: unknown;
    if (typeof value === "string") {
      variantName = value;
      hasPayload = false;
    } else if (isRecord(value) && Object.keys(value).length === 1) {
      const [key] = Object.keys(value);
      variantName = key!;
      hasPayload = true;
      payload = value[key!];
    } else {
      throw this.fail(`expected a \`${def.name}\` variant name or {"Variant": payload}`);
    }
    const entry = [...variantIndexes(def.variants)].find(([, variant]) => variant.name === variantName);
    if (entry === undefined) throw this.fail(`\`${variantName}\` is not a variant of \`${def.name}\``);
    const [index, variant] = entry;
    const isOther = def.other !== undefined && def.variants[def.other]?.name === variantName;
    this.scoped(`.${variantName}`, () => {
      switch (variant.payload) {
        case "newtype": {
          if (isOther && hasPayload) {
            const max = variant.ty.kind === "u8" ? 0xff : variant.ty.kind === "u16" ? 0xffff : 0xffff_ffff;
            w.tag(this.int(payload, 0, max), false);
            return;
          }
          if (!hasPayload) throw this.fail("variant needs a payload");
          w.tag(index, true);
          const frame = w.begin(0n);
          this.write(w, variant.ty, args, payload);
          w.end(frame);
          return;
        }
        case "unit":
          if (hasPayload && payload !== null) throw this.fail("unit variant takes no payload");
          w.tag(index, false);
          return;
        case "fields":
          if (!hasPayload) throw this.fail("variant needs a payload");
          w.tag(index, true);
          this.writeFields(w, variant.fields, args, payload);
          return;
      }
    });
  }

  private writeFields(w: Writer, fields: Fields, env: readonly Ty[], value: unknown): void {
    const list = fields.fields;
    let values: readonly (unknown | undefined)[];
    if (fields.style === "named") {
      if (!isRecord(value)) throw this.fail("expected an object");
      const unknown = Object.keys(value).find((key) => !list.some((field) => field.name === key));
      if (unknown !== undefined) throw this.fail(`unknown field \`${unknown}\``);
      values = list.map((field) => (Object.prototype.hasOwnProperty.call(value, field.name ?? "") ? value[field.name ?? ""] : undefined));
    } else {
      if (!Array.isArray(value) || value.length !== list.length) throw this.fail(`expected an array of ${list.length}`);
      values = value;
    }
    let mask = 0n;
    let bit = 0n;
    for (const [index, field] of list.entries()) {
      if (!field.optional) continue;
      if (values[index] !== undefined) mask |= 1n << bit;
      bit++;
    }
    const frame = w.begin(mask);
    for (const [index, field] of list.entries()) {
      const fieldValue = values[index];
      const segment = field.name ? `.${field.name}` : `[${index}]`;
      if (fieldValue !== undefined) this.scoped(segment, () => this.write(w, field.ty, env, fieldValue));
      else if (!field.optional) throw this.fail(`missing field \`${field.name ?? ""}\``);
    }
    w.end(frame);
  }

  read(r: Reader, ty: Ty, env: readonly Ty[]): unknown {
    switch (ty.kind) {
      case "unit":
        return null;
      case "bool": {
        const byte = r.byte();
        if (byte > 1) throw this.fail(`invalid bool byte 0x${byte.toString(16).padStart(2, "0")}`);
        return byte === 1;
      }
      case "u8":
        return r.byte();
      case "i8": {
        const byte = r.byte();
        return byte >= 0x80 ? byte - 0x100 : byte;
      }
      case "u16":
        return this.small(r.varint(), 0xffffn);
      case "u32":
        return this.small(r.varint(), 0xffff_ffffn);
      case "u64":
        return r.varint();
      case "i16":
        return Number(this.ranged(r.zigzag(), -0x8000n, 0x7fffn));
      case "i32":
        return Number(this.ranged(r.zigzag(), -0x8000_0000n, 0x7fff_ffffn));
      case "i64":
        return r.zigzag();
      case "f32":
        scratchBytes.set(r.take(4), 0);
        return scratch.getFloat32(0, true);
      case "f64":
        scratchBytes.set(r.take(8), 0);
        return scratch.getFloat64(0, true);
      case "str": {
        const bytes = r.lenPrefixed();
        try {
          return decoder.decode(bytes);
        } catch {
          throw this.fail("invalid utf-8 in string");
        }
      }
      case "bytes":
        return formatHex(r.lenPrefixed());
      case "uuid":
        return formatUuid(r.take(16));
      case "timestamp":
        return formatTimestamp(r.zigzag());
      case "option": {
        const tag = r.byte();
        if (tag === 0) return null;
        if (tag === 1) return this.read(r, ty.of, env);
        throw this.fail(`invalid bool byte 0x${tag.toString(16).padStart(2, "0")}`);
      }
      case "seq":
      case "set":
        return this.readItems(r, ty.of, env, r.length());
      case "array":
        return this.readItems(r, ty.of, env, ty.len);
      case "tuple":
        return ty.of.map((item, index) => this.scoped(`[${index}]`, () => this.read(r, item, env)));
      case "map":
        return this.readMap(r, ty.key, ty.value, env);
      case "named": {
        const args = (ty.args ?? []).map((arg) => substitute(arg, env));
        return this.readNamed(r, ty.name, args);
      }
      case "param": {
        const bound = env[ty.index];
        if (bound === undefined) throw this.fail(`unbound parameter $${ty.index}`);
        return this.read(r, bound, []);
      }
    }
  }

  private small(value: bigint, max: bigint): number {
    return Number(this.ranged(value, 0n, max));
  }

  private ranged(value: bigint, min: bigint, max: bigint): bigint {
    if (value < min || value > max) throw this.fail("varint does not fit its type");
    return value;
  }

  private readItems(r: Reader, ty: Ty, env: readonly Ty[], count: number): unknown[] {
    const items: unknown[] = new Array(count);
    for (let index = 0; index < count; index++) {
      items[index] = this.scoped(`[${index}]`, () => this.read(r, ty, env));
    }
    return items;
  }

  private readMap(r: Reader, key: Ty, value: Ty, env: readonly Ty[]): unknown {
    const count = r.length();
    if (substitute(key, env).kind === "str") {
      const entries: Record<string, unknown> = Object.create(null);
      for (let index = 0; index < count; index++) {
        const name = this.read(r, key, env) as string;
        entries[name] = this.scoped(`.${name}`, () => this.read(r, value, env));
      }
      return entries;
    }
    const pairs: unknown[] = new Array(count);
    for (let index = 0; index < count; index++) {
      pairs[index] = this.scoped(`[${index}]`, () => [this.read(r, key, env), this.read(r, value, env)]);
    }
    return pairs;
  }

  private readNamed(r: Reader, name: string, args: readonly Ty[]): unknown {
    const def = this.def(name);
    switch (def.kind) {
      case "newtype":
        return this.read(r, def.ty, args);
      case "struct":
        return this.readFields(r, def.fields, args);
      case "enum":
        return this.readEnum(r, def, args);
    }
  }

  private readEnum(r: Reader, def: Extract<Def, { kind: "enum" }>, args: readonly Ty[]): unknown {
    const [index, hasPayload] = r.tag();
    const variants = [...variantIndexes(def.variants)];
    const position = variants.findIndex(
      ([known], position) => known === index && !(position === def.other && def.variants[position]!.payload !== "unit"),
    );
    if (position < 0) {
      if (hasPayload) r.skipFrame();
      const fallback = def.other === undefined ? undefined : def.variants[def.other];
      if (fallback !== undefined) return fallback.payload === "unit" ? fallback.name : { [fallback.name]: index };
      throw this.fail(`\`${def.name}\` has no variant ${index}`);
    }
    const variant = def.variants[position]!;
    return this.scoped(`.${variant.name}`, () => {
      if (variant.payload === "unit") {
        if (hasPayload) throw this.fail("unexpected payload on a unit variant");
        return variant.name;
      }
      if (!hasPayload) throw this.fail("missing payload");
      const payload =
        variant.payload === "newtype"
          ? this.read(r.frame()[0], variant.ty, args)
          : this.readFields(r, variant.fields, args);
      return { [variant.name]: payload };
    });
  }

  private readFields(r: Reader, fields: Fields, env: readonly Ty[]): unknown {
    const [body, mask] = r.frame();
    const list = fields.fields;
    const named: Record<string, unknown> = Object.create(null);
    const items: unknown[] = new Array(list.length);
    let bit = 0n;
    for (const [index, field] of list.entries()) {
      const segment = field.name ? `.${field.name}` : `[${index}]`;
      const present = !field.optional || (mask & (1n << bit)) !== 0n;
      if (field.optional) bit++;
      if (!present) {
        if (fields.style === "tuple") items[index] = undefined;
        continue;
      }
      const value =
        field.default && body.isEmpty
          ? zeroValue(substitute(field.ty, env))
          : this.scoped(segment, () => this.read(body, field.ty, env));
      if (fields.style === "named") named[field.name ?? ""] = value;
      else items[index] = value;
    }
    return fields.style === "named" ? named : items;
  }
}
