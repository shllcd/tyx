# tyx wire format, v0

A message is a single value encoded with no header. The format carries no field names, no variant names and no
type tags: a reader must know the schema (the IR) to decode it.

## Primitives

| IR kind     | Encoding                                                         |
|-------------|------------------------------------------------------------------|
| `unit`      | nothing                                                          |
| `bool`      | one byte, `0` or `1`; anything else is an error                  |
| `u8`, `i8`  | one raw byte (two's complement for `i8`)                         |
| `u16..u64`  | unsigned LEB128 varint                                           |
| `i16..i64`  | zigzag (`(n << 1) ^ (n >> bits-1)`), then LEB128 varint          |
| `f32`/`f64` | IEEE 754, little endian, 4/8 bytes                               |
| `str`       | varint byte length, then UTF-8 bytes                             |
| `bytes`     | varint byte length, then the bytes (the same as `seq` of `u8`)   |
| `uuid`      | 16 raw bytes, RFC 9562 byte order                                |
| `timestamp` | `i64` milliseconds since the Unix epoch, UTC                     |

Readers must accept non-minimal varints (e.g. `0x80 0x00` for zero); writers should emit minimal ones.
A varint wider than its type is an error.

## Containers

| IR kind        | Encoding                                                     |
|----------------|--------------------------------------------------------------|
| `option`       | byte `0` (none) or `1` followed by the value                 |
| `seq`, `set`   | varint count, then the items                                 |
| `map`          | varint count, then key/value pairs                           |
| `array` (N)    | exactly N items, no count                                    |
| `tuple`        | the items in order, no count                                 |

`option` above is the standalone form. Struct fields of type `Option<T>` use the presence mask instead.

## Frames

Structs and enum payloads are framed so readers can skip data they do not know:

```
frame  = header [mask] body
header = varint((len << 1) | has_mask)
mask   = varint of at most 64 bits, present only when has_mask = 1
```

`len` counts every byte after the header, the mask included. After decoding the fields it knows, a reader jumps
to the end of the frame, ignoring trailing bytes written by a newer schema.

## Structs

- **Newtype** (`struct Id(Uuid)`): transparent, encoded exactly as its inner value.
- **Struct** (named fields, tuple fields or none): one frame. Fields are written in declaration order.

Fields typed `Option<T>` are *optional*: each takes the next bit of the mask (bit 0 = first optional field) and
its value is written only when present. A writer omits the mask when no optional field is present. At most 64
optional fields per struct.

A field marked `default` may be missing at the end of the frame; the reader then uses the type's zero value
(`0`, `false`, `""`, empty `seq`/`set`/`map`). An optional field missing at the end of the frame is absent.

## Enums

```
enum    = varint((index << 1) | has_payload) [frame]
```

`index` is the variant's pinned index (`A = 7`) or, unpinned, the previous variant's index plus one (0 for the
first), as Rust numbers discriminants; indexes are unique. Unit variants have no payload. Newtype variants frame
their inner value (no mask); tuple and struct variants frame their fields exactly like a struct.

A reader meeting an unknown index skips the payload frame (if any) and decodes the variant marked `other`, or
fails when there is none. An `other` variant is a unit or holds the unknown index (`u8`, `u16` or `u32`); the
latter is written back as a unit variant with that index, so the payload of an unknown data variant is lost.

## Evolution

Until a schema is frozen everything may change. After freezing, these changes are compatible:

- appending an optional or `default` field to a struct or a struct/tuple variant;
- adding a variant under a new index to an enum whose readers have an `other` variant;
- `seq` of `u8` becoming `bytes` and back.

Anything else (reordering, removing, retyping, turning a unit variant into a data one) is breaking.

A reader that rewrites what it decoded keeps newer fields by holding on to the mask bits above its own optional
fields and the bytes after its last field, and writing both back: the bits into its mask, the bytes after its
fields in the same frame.
