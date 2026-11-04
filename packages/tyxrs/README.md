# tyxrs

TypeScript runtime for the [tyx](https://github.com/shllcd/tyx) wire format. It reads the schema a Rust
program exports as IR, encodes and decodes values against it, and generates TypeScript types for it.
It runs in Node and browsers.

```sh
pnpm add tyxrs
```

## Usage

Export the schema from Rust:

```rust
let schema = tyx::schema()?;
std::fs::write("schema.json", schema.to_json())?;
```

Then encode and decode by type name:

```ts
import { Codec } from "tyxrs";
import schema from "./schema.json" with { type: "json" };

const codec = new Codec(schema);

const bytes = codec.encode("User", { id: 1n, name: "Ada" });
const user = codec.decode("User", bytes);
```

`encode` and `decode` take a type name or an IR type such as `{ kind: "seq", of: { kind: "u32" } }`.

## Types

`generateTypes` emits declarations for every type in the schema plus a `TyxTypes` map:

```ts
import { writeFileSync } from "node:fs";
import { generateTypes } from "tyxrs";
import schema from "./schema.json" with { type: "json" };

writeFileSync("types.ts", generateTypes(schema));
```

Pass the map to `Codec` and both directions are typed:

```ts
import { Codec } from "tyxrs";
import type { TyxTypes } from "./types.js";

const codec = new Codec<TyxTypes>(schema);
const user = codec.decode("User", bytes); // User
codec.encode("User", user);
```

`u64` and `i64` become `bigint`; a fixed-size array becomes `TyxFixedArray<T, N>`; a string-keyed map becomes
`{ [key: string]: V }` and any other map `TyxMap<K, V>`, an array of pairs. An enum becomes a union of `"Variant"`
and `{ Variant: payload }`. Generic types are generated but stay out of `TyxTypes`; pass an IR `Ty` for those and
the result is `unknown`.

## Browser

The runtime uses only web-standard APIs (`TextEncoder`, `TextDecoder`, `DataView`, `Uint8Array`, `bigint`) and
targets ES2020. It has no Node dependencies, so it works through a bundler or as a plain ES module:

```ts
import { Codec } from "tyxrs";

const schema = await (await fetch("/schema.json")).json();
const bytes = new Uint8Array(await (await fetch("/user.bin")).arrayBuffer());

const codec = new Codec(schema);
const user = codec.decode("User", bytes);
```

`generateTypes` is platform-neutral as well: it takes the schema and returns the TypeScript source as a string.

## Value mapping

| IR                         | TypeScript                                                     |
| -------------------------- | -------------------------------------------------------------- |
| `unit`                     | `null`                                                         |
| `bool`                     | `boolean`                                                      |
| `u8`, `u16`, `u32`, `i8`, `i16`, `i32` | `number`                                            |
| `u64`, `i64`               | `bigint`; `encode` also takes a `number` that is a safe integer |
| `f32`, `f64`               | `number`                                                       |
| `str`                      | `string`                                                       |
| `bytes`                    | lowercase hex `string`                                         |
| `uuid`                     | hyphenated `string`                                            |
| `timestamp`                | RFC 3339 `string`, decoded in UTC                              |
| `option`                   | `null` or the value; `undefined` encodes as none               |
| `seq`, `set`               | array                                                          |
| `array`                    | array of exactly `len` items                                   |
| `tuple`                    | array of exactly its item count                                |
| `map` with `str` keys      | object, `null`-prototype on decode                             |
| other `map`                | array of `[key, value]` pairs                                  |
| struct                     | object for named fields, array for tuple fields                |
| newtype                    | its inner value                                                |
| enum                       | `"Variant"` or `{ Variant: payload }`                          |

An enum with an `other` variant holding the index decodes an unknown index as `{ "Other": index }`; one with a
unit `other` decodes it as `"Other"`. Enums whose `other` variant holds the index re-encode that value with the
index as the variant tag.

Decoding a struct ignores bytes after its known fields, so a newer writer's fields do not break an older reader.
Encoding rejects unknown fields, missing required fields and values out of range.

## Errors

Every failure throws a `TyxError` whose `path` points at the value, for example `.tags[2]`:

```ts
try {
  codec.decode("User", bytes);
} catch (error) {
  console.error(error.path, error.message);
}
```

## License

MIT
