import type { Def, Field, Fields, Schema, Ty, Variant } from "./ir.js";
import { Reader, TyxError, Writer } from "./wire.js";

const I64_MIN = -0x8000_0000_0000_0000n;
const I64_MAX = 0x7fff_ffff_ffff_ffffn;
const U64_MAX = 0xffff_ffff_ffff_ffffn;
const MAX_NUMBER_MASK_BITS = 52;
const MAX_REUSED_WRITER_CAPACITY = 0x1_0000;

type Encode = (w: Writer, value: unknown) => void;
type Decode = (r: Reader) => unknown;
type EnumDef = Extract<Def, { kind: "enum" }>;

/**
 * Encodes and decodes values through an IR schema. `u64` and `i64` are `bigint`; every other
 * integer is `number`. See the README for the full value mapping.
 */
export class Codec<Types extends object = Record<string, unknown>> {
  private readonly compiler: Compiler;
  private readonly tyEncoders = new WeakMap<Ty, Encode>();
  private readonly tyDecoders = new WeakMap<Ty, Decode>();
  private spareWriter: Writer | undefined = new Writer();

  constructor(schema: Schema) {
    const types = new Map<string, Def>();
    for (const def of schema.types) {
      if (types.has(def.name)) throw new TyxError(`type \`${def.name}\` is defined twice`);
      types.set(def.name, def);
    }
    this.compiler = new Compiler(types);
  }

  encode<K extends Extract<keyof Types, string>>(name: K, value: Types[K]): Uint8Array;
  encode(ty: Ty, value: unknown): Uint8Array;
  encode(ty: Ty | string, value: unknown): Uint8Array {
    const encode = this.encoderFor(ty);
    const w = this.spareWriter ?? new Writer();
    this.spareWriter = undefined;
    try {
      w.clear();
      encode(w, value);
      return w.intoBytes();
    } finally {
      if (w.capacity <= MAX_REUSED_WRITER_CAPACITY) this.spareWriter = w;
    }
  }

  decode<K extends Extract<keyof Types, string>>(name: K, bytes: Uint8Array): Types[K];
  decode(ty: Ty, bytes: Uint8Array): unknown;
  decode(ty: Ty | string, bytes: Uint8Array): unknown {
    const r = new Reader(bytes);
    const value = this.decoderFor(ty)(r);
    if (!r.isEmpty) throw new TyxError("trailing bytes after the value");
    return value;
  }

  private encoderFor(ty: Ty | string): Encode {
    if (typeof ty === "string") return this.compiler.namedEncoder(ty, []);
    let encode = this.tyEncoders.get(ty);
    if (encode === undefined) {
      encode = this.compiler.encoder(ty, []);
      this.tyEncoders.set(ty, encode);
    }
    return encode;
  }

  private decoderFor(ty: Ty | string): Decode {
    if (typeof ty === "string") return this.compiler.namedDecoder(ty, []);
    let decode = this.tyDecoders.get(ty);
    if (decode === undefined) {
      decode = this.compiler.decoder(ty, []);
      this.tyDecoders.set(ty, decode);
    }
    return decode;
  }
}

function* variantIndexes(variants: readonly Variant[]): Generator<[number, Variant]> {
  let next = 0;
  for (const variant of variants) {
    const index = variant.index ?? next;
    next = (index + 1) >>> 0;
    yield [index, variant];
  }
}

function maskBits(fields: readonly Field[]): number[] {
  let next = 0;
  return fields.map((field) => (field.optional ? next++ : -1));
}

function maxHeldIndex(variant: Variant): number | undefined {
  if (variant.payload !== "newtype") return undefined;
  switch (variant.ty.kind) {
    case "u8":
      return 0xff;
    case "u16":
      return 0xffff;
    case "u32":
      return 0xffff_ffff;
    default:
      return undefined;
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

function zeroValue(ty: Ty): () => unknown {
  switch (ty.kind) {
    case "bool":
      return () => false;
    case "u8":
    case "u16":
    case "u32":
    case "i8":
    case "i16":
    case "i32":
    case "f32":
    case "f64":
      return () => 0;
    case "u64":
    case "i64":
      return () => 0n;
    case "str":
    case "bytes":
      return () => "";
    case "seq":
    case "set":
      return () => [];
    case "map":
      return ty.key.kind === "str" ? () => Object.create(null) : () => [];
    default:
      return () => null;
  }
}

function within(error: unknown, segment: string): unknown {
  if (error instanceof TyxError) error.path = segment + error.path;
  return error;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const hasOwn = Object.prototype.hasOwnProperty;

const nativeIsWellFormed = (String.prototype as { isWellFormed?: (this: string) => boolean }).isWellFormed;

function isWellFormed(value: string): boolean {
  if (nativeIsWellFormed !== undefined) return nativeIsWellFormed.call(value);
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

function int(value: unknown, min: number, max: number): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < min || value > max) {
    throw new TyxError(`expected an integer in ${min}..=${max}, got ${show(value)}`);
  }
  return value;
}

function bigint(value: unknown, min: bigint, max: bigint): bigint {
  const result =
    typeof value === "bigint" ? value : typeof value === "number" && Number.isSafeInteger(value) ? BigInt(value) : undefined;
  if (result === undefined || result < min || result > max) {
    throw new TyxError(`expected an integer in ${min}..=${max}, got ${show(value)}`);
  }
  return result;
}

function string(value: unknown): string {
  if (typeof value !== "string") throw new TyxError("expected a string");
  if (!isWellFormed(value)) throw new TyxError("expected a well-formed string");
  return value;
}

function float(value: unknown): number {
  if (typeof value !== "number") throw new TyxError("expected a number");
  return value;
}

const HEX_DIGITS = Array.from({ length: 256 }, (_, byte) => byte.toString(16).padStart(2, "0"));
const HEX_VALUES = new Int8Array(128).fill(-1);
for (let digit = 0; digit < 16; digit++) {
  HEX_VALUES[digit.toString(16).charCodeAt(0)] = digit;
  HEX_VALUES[digit.toString(16).toUpperCase().charCodeAt(0)] = digit;
}
const HYPHEN = 0x2d;
const uuidScratch = new Uint8Array(16);

function hexValue(code: number): number {
  return code < 0x80 ? HEX_VALUES[code]! : -1;
}

function parseHex(value: unknown): Uint8Array {
  if (typeof value !== "string" || value.length % 2 !== 0) throw new TyxError("expected a hex string");
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < bytes.length; index++) {
    const high = hexValue(value.charCodeAt(index * 2));
    const low = hexValue(value.charCodeAt(index * 2 + 1));
    if (high < 0 || low < 0) throw new TyxError("expected a hex string");
    bytes[index] = high * 16 + low;
  }
  return bytes;
}

function formatHex(bytes: Uint8Array): string {
  let text = "";
  for (let index = 0; index < bytes.length; index++) text += HEX_DIGITS[bytes[index]!];
  return text;
}

function parseUuid(value: unknown): Uint8Array {
  if (typeof value !== "string" || value.length !== 36) throw new TyxError("expected a hyphenated uuid");
  const bytes = uuidScratch;
  let nibble = 0;
  let isHex = true;
  for (let index = 0; index < 36; index++) {
    const code = value.charCodeAt(index);
    if (code === HYPHEN) continue;
    const digit = hexValue(code);
    if (digit < 0) isHex = false;
    if (nibble < 32) bytes[nibble >> 1] = (nibble & 1) === 0 ? digit << 4 : bytes[nibble >> 1]! | digit;
    nibble++;
  }
  if (nibble !== 32) throw new TyxError("expected a hyphenated uuid");
  if (!isHex) throw new TyxError("expected a hex string");
  return bytes;
}

function formatUuid(bytes: Uint8Array): string {
  let text = "";
  for (let index = 0; index < 16; index++) {
    if (index === 4 || index === 6 || index === 8 || index === 10) text += "-";
    text += HEX_DIGITS[bytes[index]!];
  }
  return text;
}

const RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/;

function parseTimestamp(value: unknown): number {
  if (typeof value !== "string" || !RFC3339.test(value)) throw new TyxError("expected an RFC 3339 timestamp");
  const millis = Date.parse(value);
  if (Number.isNaN(millis)) throw new TyxError("expected an RFC 3339 timestamp");
  return millis;
}

const MILLIS_PER_DAY = 86_400_000;
const MIN_TIMESTAMP_MILLIS = -62_167_219_200_000;
const MAX_TIMESTAMP_MILLIS = 253_402_300_799_999;

function pad2(value: number): string {
  return value < 10 ? `0${value}` : `${value}`;
}

function pad4(value: number): string {
  return value < 10 ? `000${value}` : value < 100 ? `00${value}` : value < 1000 ? `0${value}` : `${value}`;
}

function formatTimestamp(millis: number): string {
  if (!(millis >= MIN_TIMESTAMP_MILLIS && millis <= MAX_TIMESTAMP_MILLIS)) throw new TyxError("timestamp out of range");
  const days = Math.floor(millis / MILLIS_PER_DAY);
  const millisOfDay = millis - days * MILLIS_PER_DAY;
  const shifted = days + 719_468;
  const era = Math.floor(shifted / 146_097);
  const dayOfEra = shifted - era * 146_097;
  const yearOfEra = Math.floor((dayOfEra - Math.floor(dayOfEra / 1460) + Math.floor(dayOfEra / 36_524) - Math.floor(dayOfEra / 146_096)) / 365);
  const dayOfYear = dayOfEra - (365 * yearOfEra + Math.floor(yearOfEra / 4) - Math.floor(yearOfEra / 100));
  const monthFromMarch = Math.floor((5 * dayOfYear + 2) / 153);
  const day = dayOfYear - Math.floor((153 * monthFromMarch + 2) / 5) + 1;
  const month = monthFromMarch < 10 ? monthFromMarch + 3 : monthFromMarch - 9;
  const year = yearOfEra + era * 400 + (month <= 2 ? 1 : 0);
  const seconds = Math.floor(millisOfDay / 1000);
  const fraction = millisOfDay - seconds * 1000;
  const text = `${pad4(year)}-${pad2(month)}-${pad2(day)}T${pad2(Math.floor(seconds / 3600))}:${pad2(Math.floor(seconds / 60) % 60)}:${pad2(seconds % 60)}`;
  if (fraction === 0) return `${text}Z`;
  if (fraction % 100 === 0) return `${text}.${fraction / 100}Z`;
  if (fraction % 10 === 0) return `${text}.${pad2(fraction / 10)}Z`;
  return `${text}.${fraction < 10 ? "00" : fraction < 100 ? "0" : ""}${fraction}Z`;
}

function unsigned(max: number): Decode {
  return (r) => {
    const value = r.varintNumber();
    if (value > max) throw new TyxError("varint does not fit its type");
    return value;
  };
}

function signed(min: number, max: number): Decode {
  return (r) => {
    const value = r.zigzagNumber();
    if (value < min || value > max) throw new TyxError("varint does not fit its type");
    return value;
  };
}

function maskHas(mask: number | bigint, bit: number): boolean {
  if (typeof mask === "bigint") return ((mask >> BigInt(bit)) & 1n) === 1n;
  if (bit < 32) return ((mask >>> bit) & 1) === 1;
  return Math.floor(mask / 2 ** bit) % 2 === 1;
}

function unbound(index: number): () => never {
  return () => {
    throw new TyxError(`unbound parameter $${index}`);
  };
}

class Compiler {
  private readonly types: Map<string, Def>;
  private readonly namedEncoders = new Map<string, Encode>();
  private readonly namedDecoders = new Map<string, Decode>();

  constructor(types: Map<string, Def>) {
    this.types = types;
  }

  private def(name: string): Def {
    const def = this.types.get(name);
    if (def === undefined) throw new TyxError(`unknown type \`${name}\``);
    return def;
  }

  namedEncoder(name: string, args: readonly Ty[]): Encode {
    const key = args.length === 0 ? name : `${name}${JSON.stringify(args)}`;
    let encode = this.namedEncoders.get(key);
    if (encode === undefined) {
      let compiled: Encode | undefined;
      encode = (w, value) => (compiled ??= this.compileNamedEncoder(name, args))(w, value);
      this.namedEncoders.set(key, encode);
    }
    return encode;
  }

  namedDecoder(name: string, args: readonly Ty[]): Decode {
    const key = args.length === 0 ? name : `${name}${JSON.stringify(args)}`;
    let decode = this.namedDecoders.get(key);
    if (decode === undefined) {
      let compiled: Decode | undefined;
      decode = (r) => (compiled ??= this.compileNamedDecoder(name, args))(r);
      this.namedDecoders.set(key, decode);
    }
    return decode;
  }

  private compileNamedEncoder(name: string, args: readonly Ty[]): Encode {
    const def = this.def(name);
    switch (def.kind) {
      case "newtype":
        return this.encoder(def.ty, args);
      case "struct":
        return this.fieldsEncoder(def.fields, args);
      case "enum":
        return this.enumEncoder(def, args);
    }
  }

  private compileNamedDecoder(name: string, args: readonly Ty[]): Decode {
    const def = this.def(name);
    switch (def.kind) {
      case "newtype":
        return this.decoder(def.ty, args);
      case "struct":
        return this.fieldsDecoder(def.fields, args);
      case "enum":
        return this.enumDecoder(def, args);
    }
  }

  encoder(ty: Ty, env: readonly Ty[]): Encode {
    switch (ty.kind) {
      case "unit":
        return () => {};
      case "bool":
        return (w, value) => {
          if (typeof value !== "boolean") throw new TyxError("expected a boolean");
          w.byte(value ? 1 : 0);
        };
      case "u8":
        return (w, value) => w.byte(int(value, 0, 0xff));
      case "i8":
        return (w, value) => w.byte(int(value, -0x80, 0x7f) & 0xff);
      case "u16":
        return (w, value) => w.uvarint(int(value, 0, 0xffff));
      case "u32":
        return (w, value) => w.uvarint(int(value, 0, 0xffff_ffff));
      case "u64":
        return (w, value) => {
          if (typeof value === "number" && Number.isSafeInteger(value) && value >= 0) w.uvarint(value);
          else w.varint(bigint(value, 0n, U64_MAX));
        };
      case "i16":
        return (w, value) => w.zigzagNumber(int(value, -0x8000, 0x7fff));
      case "i32":
        return (w, value) => w.zigzagNumber(int(value, -0x8000_0000, 0x7fff_ffff));
      case "i64":
        return (w, value) => {
          if (typeof value === "number" && Number.isSafeInteger(value)) w.zigzagNumber(value);
          else w.zigzag(bigint(value, I64_MIN, I64_MAX));
        };
      case "f32":
        return (w, value) => w.f32(float(value));
      case "f64":
        return (w, value) => w.f64(float(value));
      case "str":
        return (w, value) => w.str(string(value));
      case "bytes":
        return (w, value) => w.lenPrefixed(parseHex(value));
      case "uuid":
        return (w, value) => w.bytes(parseUuid(value));
      case "timestamp":
        return (w, value) => w.zigzagNumber(parseTimestamp(value));
      case "option": {
        const inner = this.encoder(ty.of, env);
        return (w, value) => {
          if (value === null || value === undefined) {
            w.byte(0);
          } else {
            w.byte(1);
            inner(w, value);
          }
        };
      }
      case "seq":
      case "set": {
        const item = this.encoder(ty.of, env);
        return (w, value) => {
          if (!Array.isArray(value)) throw new TyxError("expected an array");
          w.uvarint(value.length);
          writeItems(w, item, value);
        };
      }
      case "array": {
        const item = this.encoder(ty.of, env);
        const len = ty.len;
        return (w, value) => {
          if (!Array.isArray(value) || value.length !== len) throw new TyxError(`expected an array of ${len}`);
          writeItems(w, item, value);
        };
      }
      case "tuple": {
        const items = ty.of.map((item) => this.encoder(item, env));
        return (w, value) => {
          if (!Array.isArray(value) || value.length !== items.length) throw new TyxError(`expected an array of ${items.length}`);
          for (let index = 0; index < items.length; index++) {
            try {
              items[index]!(w, value[index]);
            } catch (error) {
              throw within(error, `[${index}]`);
            }
          }
        };
      }
      case "map":
        return this.mapEncoder(ty.key, ty.value, env);
      case "named":
        return this.namedEncoder(
          ty.name,
          (ty.args ?? []).map((arg) => substitute(arg, env)),
        );
      case "param": {
        const bound = env[ty.index];
        return bound === undefined ? unbound(ty.index) : this.encoder(bound, []);
      }
    }
  }

  private mapEncoder(key: Ty, value: Ty, env: readonly Ty[]): Encode {
    const hasStringKeys = substitute(key, env).kind === "str";
    const encodeKey = this.encoder(key, env);
    const encodeValue = this.encoder(value, env);
    return (w, map) => {
      if (isRecord(map)) {
        if (!hasStringKeys) throw new TyxError("only maps with string keys are objects; use [[key, value], ...]");
        const names = Object.keys(map);
        w.uvarint(names.length);
        for (const name of names) {
          w.str(name);
          try {
            encodeValue(w, map[name]);
          } catch (error) {
            throw within(error, `.${name}`);
          }
        }
        return;
      }
      if (!Array.isArray(map)) throw new TyxError("expected an object or an array of [key, value]");
      w.uvarint(map.length);
      for (let index = 0; index < map.length; index++) {
        try {
          const pair: unknown = map[index];
          if (!Array.isArray(pair) || pair.length !== 2) throw new TyxError("expected [key, value]");
          encodeKey(w, pair[0]);
          encodeValue(w, pair[1]);
        } catch (error) {
          throw within(error, `[${index}]`);
        }
      }
    };
  }

  private enumEncoder(def: EnumDef, args: readonly Ty[]): Encode {
    type Write = (w: Writer, hasPayload: boolean, payload: unknown) => void;
    const otherName = def.other === undefined ? undefined : def.variants[def.other]?.name;
    const writers = new Map<string, Write>();
    const indexed = [...variantIndexes(def.variants)];
    const knownIndexes = new Set(indexed.filter(([, variant]) => variant.name !== otherName).map(([index]) => index));
    for (const [index, variant] of indexed) {
      if (writers.has(variant.name)) continue;
      const heldIndexMax = otherName === variant.name ? maxHeldIndex(variant) : undefined;
      writers.set(variant.name, this.variantWriter(index, variant, heldIndexMax, knownIndexes, args));
    }
    return (w, value) => {
      let variantName: string;
      let hasPayload: boolean;
      let payload: unknown;
      if (typeof value === "string") {
        variantName = value;
        hasPayload = false;
      } else {
        const key = isRecord(value) ? soleKey(value) : undefined;
        if (key === undefined) throw new TyxError(`expected a \`${def.name}\` variant name or {"Variant": payload}`);
        variantName = key;
        hasPayload = true;
        payload = (value as Record<string, unknown>)[key];
      }
      const write = writers.get(variantName);
      if (write === undefined) throw new TyxError(`\`${variantName}\` is not a variant of \`${def.name}\``);
      try {
        write(w, hasPayload, payload);
      } catch (error) {
        throw within(error, `.${variantName}`);
      }
    };
  }

  private variantWriter(
    index: number,
    variant: Variant,
    heldIndexMax: number | undefined,
    knownIndexes: ReadonlySet<number>,
    args: readonly Ty[],
  ): (w: Writer, hasPayload: boolean, payload: unknown) => void {
    switch (variant.payload) {
      case "newtype": {
        const inner = this.encoder(variant.ty, args);
        return (w, hasPayload, payload) => {
          if (heldIndexMax !== undefined && hasPayload) {
            const heldIndex = int(payload, 0, heldIndexMax);
            if (knownIndexes.has(heldIndex)) throw new TyxError(`index ${heldIndex} belongs to a known variant`);
            w.tag(heldIndex, false);
            return;
          }
          if (!hasPayload) throw new TyxError("variant needs a payload");
          w.tag(index, true);
          const frame = w.begin(0);
          inner(w, payload);
          w.end(frame);
        };
      }
      case "unit":
        return (w, hasPayload, payload) => {
          if (hasPayload && payload !== null) throw new TyxError("unit variant takes no payload");
          w.tag(index, false);
        };
      case "fields": {
        const fields = this.fieldsEncoder(variant.fields, args);
        return (w, hasPayload, payload) => {
          if (!hasPayload) throw new TyxError("variant needs a payload");
          w.tag(index, true);
          fields(w, payload);
        };
      }
    }
  }

  private fieldsEncoder(fields: Fields, env: readonly Ty[]): Encode {
    const list = fields.fields;
    const count = list.length;
    const encoders = list.map((field) => this.encoder(field.ty, env));
    const names = list.map((field) => field.name ?? "");
    const optional = list.map((field) => field.optional === true);
    const optionalCount = optional.filter(Boolean).length;
    const isWide = optionalCount > MAX_NUMBER_MASK_BITS;
    const bits = maskBits(list);
    const known = new Set(list.flatMap((field) => (field.name === undefined ? [] : [field.name])));
    const isNamed = fields.style === "named";

    const segment = (index: number) => (list[index]!.name ? `.${list[index]!.name}` : `[${index}]`);

    const collect = (value: unknown): readonly unknown[] => {
      if (isNamed) {
        if (!isRecord(value)) throw new TyxError("expected an object");
        for (const key in value) {
          if (hasOwn.call(value, key) && !known.has(key)) throw new TyxError(`unknown field \`${key}\``);
        }
        const values: unknown[] = new Array(count);
        for (let index = 0; index < count; index++) {
          const name = names[index]!;
          values[index] = hasOwn.call(value, name) ? value[name] : undefined;
        }
        return values;
      }
      if (!Array.isArray(value) || value.length !== count) throw new TyxError(`expected an array of ${count}`);
      return value.map((item, index) => (item === null && optional[index] ? undefined : item));
    };

    const maskOf = (values: readonly unknown[]): number | bigint => {
      if (optionalCount === 0) return 0;
      if (isWide) {
        let mask = 0n;
        for (let index = 0; index < count; index++) {
          if (optional[index] && values[index] !== undefined) mask |= 1n << BigInt(bits[index]!);
        }
        return mask;
      }
      let mask = 0;
      for (let index = 0; index < count; index++) {
        if (optional[index] && values[index] !== undefined) mask += 2 ** bits[index]!;
      }
      return mask;
    };

    return (w, value) => {
      const values = collect(value);
      const frame = w.begin(maskOf(values));
      for (let index = 0; index < count; index++) {
        const fieldValue = values[index];
        if (fieldValue !== undefined) {
          try {
            encoders[index]!(w, fieldValue);
          } catch (error) {
            throw within(error, segment(index));
          }
        } else if (!optional[index]) {
          throw new TyxError(`missing field \`${list[index]!.name ?? ""}\``);
        }
      }
      w.end(frame);
    };
  }

  decoder(ty: Ty, env: readonly Ty[]): Decode {
    switch (ty.kind) {
      case "unit":
        return () => null;
      case "bool":
        return (r) => {
          const byte = r.byte();
          if (byte > 1) throw new TyxError(`invalid bool byte 0x${byte.toString(16).padStart(2, "0")}`);
          return byte === 1;
        };
      case "u8":
        return (r) => r.byte();
      case "i8":
        return (r) => {
          const byte = r.byte();
          return byte >= 0x80 ? byte - 0x100 : byte;
        };
      case "u16":
        return unsigned(0xffff);
      case "u32":
        return unsigned(0xffff_ffff);
      case "u64":
        return (r) => r.varint();
      case "i16":
        return signed(-0x8000, 0x7fff);
      case "i32":
        return signed(-0x8000_0000, 0x7fff_ffff);
      case "i64":
        return (r) => r.zigzag();
      case "f32":
        return (r) => r.f32();
      case "f64":
        return (r) => r.f64();
      case "str":
        return (r) => r.str();
      case "bytes":
        return (r) => formatHex(r.lenPrefixed());
      case "uuid":
        return (r) => formatUuid(r.take(16));
      case "timestamp":
        return (r) => formatTimestamp(r.zigzagNumber());
      case "option": {
        const inner = this.decoder(ty.of, env);
        return (r) => {
          const tag = r.byte();
          if (tag === 0) return null;
          if (tag === 1) return inner(r);
          throw new TyxError(`invalid bool byte 0x${tag.toString(16).padStart(2, "0")}`);
        };
      }
      case "seq":
      case "set": {
        const item = this.decoder(ty.of, env);
        return (r) => readItems(r, item, r.length());
      }
      case "array": {
        const item = this.decoder(ty.of, env);
        const len = ty.len;
        return (r) => readItems(r, item, len);
      }
      case "tuple": {
        const items = ty.of.map((item) => this.decoder(item, env));
        return (r) => {
          const values: unknown[] = [];
          for (let index = 0; index < items.length; index++) {
            try {
              values.push(items[index]!(r));
            } catch (error) {
              throw within(error, `[${index}]`);
            }
          }
          return values;
        };
      }
      case "map":
        return this.mapDecoder(ty.key, ty.value, env);
      case "named":
        return this.namedDecoder(
          ty.name,
          (ty.args ?? []).map((arg) => substitute(arg, env)),
        );
      case "param": {
        const bound = env[ty.index];
        return bound === undefined ? unbound(ty.index) : this.decoder(bound, []);
      }
    }
  }

  private mapDecoder(key: Ty, value: Ty, env: readonly Ty[]): Decode {
    const hasStringKeys = substitute(key, env).kind === "str";
    const decodeKey = this.decoder(key, env);
    const decodeValue = this.decoder(value, env);
    if (hasStringKeys) {
      return (r) => {
        const count = r.length();
        const entries: Record<string, unknown> = Object.create(null);
        for (let index = 0; index < count; index++) {
          const name = decodeKey(r) as string;
          try {
            entries[name] = decodeValue(r);
          } catch (error) {
            throw within(error, `.${name}`);
          }
        }
        return entries;
      };
    }
    return (r) => {
      const count = r.length();
      const pairs: unknown[] = new Array(count);
      for (let index = 0; index < count; index++) {
        try {
          pairs[index] = [decodeKey(r), decodeValue(r)];
        } catch (error) {
          throw within(error, `[${index}]`);
        }
      }
      return pairs;
    };
  }

  private enumDecoder(def: EnumDef, args: readonly Ty[]): Decode {
    const positions = new Map<number, number>();
    let position = 0;
    for (const [index, variant] of variantIndexes(def.variants)) {
      const isOtherWithIndex = position === def.other && maxHeldIndex(variant) !== undefined;
      if (!isOtherWithIndex && !positions.has(index)) positions.set(index, position);
      position++;
    }
    const readers = def.variants.map((variant) => this.variantReader(variant, args));
    const fallback = def.other === undefined ? undefined : def.variants[def.other];
    return (r) => {
      const raw = r.tagRaw();
      const index = Math.floor(raw / 2);
      const hasPayload = raw % 2 === 1;
      const found = positions.get(index);
      if (found === undefined) {
        if (hasPayload) r.skipFrame();
        if (fallback !== undefined) return fallback.payload === "unit" ? fallback.name : { [fallback.name]: index };
        throw new TyxError(`\`${def.name}\` has no variant ${index}`);
      }
      try {
        return readers[found]!(r, hasPayload);
      } catch (error) {
        throw within(error, `.${def.variants[found]!.name}`);
      }
    };
  }

  private variantReader(variant: Variant, args: readonly Ty[]): (r: Reader, hasPayload: boolean) => unknown {
    const name = variant.name;
    switch (variant.payload) {
      case "unit":
        return (_, hasPayload) => {
          if (hasPayload) throw new TyxError("unexpected payload on a unit variant");
          return name;
        };
      case "newtype": {
        const inner = this.decoder(variant.ty, args);
        return (r, hasPayload) => {
          if (!hasPayload) throw new TyxError("missing payload");
          r.enterFrame();
          const payload = inner(r);
          r.leaveFrame();
          return { [name]: payload };
        };
      }
      case "fields": {
        const fields = this.fieldsDecoder(variant.fields, args);
        return (r, hasPayload) => {
          if (!hasPayload) throw new TyxError("missing payload");
          return { [name]: fields(r) };
        };
      }
    }
  }

  private fieldsDecoder(fields: Fields, env: readonly Ty[]): Decode {
    const list = fields.fields;
    const count = list.length;
    const decoders = list.map((field) => this.decoder(field.ty, env));
    const names = list.map((field) => field.name ?? "");
    const zeros = list.map((field) => (field.default ? zeroValue(substitute(field.ty, env)) : undefined));
    const bits = maskBits(list);
    const isNamed = fields.style === "named";

    const segment = (index: number) => (list[index]!.name ? `.${list[index]!.name}` : `[${index}]`);

    const read = (r: Reader, mask: number | bigint, index: number): unknown => {
      const bit = bits[index]!;
      if (bit >= 0 && !maskHas(mask, bit)) return undefined;
      const zero = zeros[index];
      if (zero !== undefined && r.isEmpty) return zero();
      try {
        return decoders[index]!(r);
      } catch (error) {
        throw within(error, segment(index));
      }
    };

    return (r) => {
      const mask = r.enterFrame();
      const decoded: Record<string, unknown> | unknown[] = isNamed ? Object.create(null) : new Array(count);
      for (let index = 0; index < count; index++) {
        const value = read(r, mask, index);
        if (!isNamed) (decoded as unknown[])[index] = value;
        else if (value !== undefined) (decoded as Record<string, unknown>)[names[index]!] = value;
      }
      r.leaveFrame();
      return decoded;
    };
  }
}

function soleKey(value: Record<string, unknown>): string | undefined {
  let sole: string | undefined;
  for (const key in value) {
    if (!hasOwn.call(value, key)) continue;
    if (sole !== undefined) return undefined;
    sole = key;
  }
  return sole;
}

function writeItems(w: Writer, item: Encode, items: readonly unknown[]): void {
  for (let index = 0; index < items.length; index++) {
    try {
      item(w, items[index]);
    } catch (error) {
      throw within(error, `[${index}]`);
    }
  }
}

function readItems(r: Reader, item: Decode, count: number): unknown[] {
  const items: unknown[] = new Array(count);
  for (let index = 0; index < count; index++) {
    try {
      items[index] = item(r);
    } catch (error) {
      throw within(error, `[${index}]`);
    }
  }
  return items;
}
