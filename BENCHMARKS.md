# tyx serialization benchmark: results

Command:

```sh
cargo xtask bench
```

Machine: Apple M3, macOS 26.6.2. Toolchain: rustc 1.98.1 (release profile: `lto = "fat"`, `codegen-units = 1`). External tools: flatc 25.12.19, capnp 1.2.0.

Crates (from `Cargo.lock`): tyx 0.0.0 (path, features `time`, `uuid`), bincode 2.0.1, bitcode 0.6.9, capnp 0.27.2, capnpc 0.27.0, ciborium 0.2.2, flatbuffers 25.12.19, pot 3.0.1, postcard 1.1.3, prost 0.14.4, rkyv 0.8.18, rmp-serde 1.3.1, serde 1.0.229, serde_json 1.0.154, time 0.3.55, uuid 1.28.0, zstd 0.14.3.

Methodology:

- Data comes from a deterministic LCG. Every format round-trips (full and `None` variants) before it is timed.
- Timing: a self-written harness that calibrates, then takes the median of 7 samples of about 12 ms each. Decode time includes dropping the value. Allocations are counted by a counting `#[global_allocator]` on one extra call.
- Encode reuses an output `Vec` where the format allows it (tyx: `tyx::encode_into`). The extra "pre-sized `Writer`" row appends into one large `Writer`.
- Times are stored as i64 Unix milliseconds in every format. serde formats use a `with` helper and rkyv uses an `ArchiveWith` wrapper.
- prost is measured on its native message types (bytes ids, i64 times, i32 enums); model conversion happens outside the timing. bitcode (derive) is measured the same way, on mirror types with i64 millis. bitcode (serde) is shown for reference only.
- flatbuffers and capnp build from and decode into the domain model. Their encode time includes copying the finished message into the output `Vec`.
- Single-machine run; no repetitions across processes. Treat differences under ~10% as noise.


## User row

| format | bytes | bytes (last_seen = None) | encode ns | encode allocs (n / B) | decode ns | decode allocs (n / B) | borrowed decode ns | borrowed allocs (n / B) |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| tyx | 61 | 54 | 18.6 | 0 / 0 | 60.5 | 1 / 29 | 30.4 | 0 / 0 |
| tyx (encode appended into pre-sized `Writer`) | 61 | | 15.4 | 0 / 0 | | | | |
| bincode 2 (serde) | 67 | 58 | 16.9 | 0 / 0 | 53.2 | 1 / 29 | 41.4 | 0 / 0 |
| postcard | 61 | 55 | 25.9 | 0 / 0 | 61.8 | 1 / 29 | 36.3 | 0 / 0 |
| bitcode (derive) | 67 | 58 | 40.2 | 0 / 0 | 83.8 | 3 / 45 | — | — |
| bitcode (serde) | 68 | 59 | 558 | 16 / 1347 | 567 | 8 / 1341 | — | — |
| rkyv | 88 | 88 | 16.7 | 0 / 0 | 47.9 | 1 / 29 | — | — |
| protobuf (prost) | 65 | 58 | 16.6 | 0 / 0 | 101 | 3 / 61 | — | — |
| flatbuffers | 104 | 96 | 35.3 | 0 / 0 | 75.6 | 1 / 29 | — | — |
| capnp | 96 | 96 | 48.6 | 1 / 64 | 62.9 | 1 / 29 | — | — |
| capnp packed | 75 | 69 | 199 | 2 / 8256 | 139 | 3 / 133 | — | — |
| pot | 108 | 102 | 130 | 2 / 288 | 276 | 4 / 509 | — | — |
| msgpack (rmp, array) | 73 | 65 | 24.5 | 0 / 0 | 75.1 | 1 / 29 | — | — |
| msgpack (rmp, named) | 105 | 97 | 26.0 | 0 / 0 | 109 | 1 / 29 | — | — |
| cbor (ciborium) | 105 | 97 | 60.8 | 0 / 0 | 289 | 1 / 29 | — | — |
| json (serde_json) | 150 | 141 | 98.5 | 0 / 0 | 182 | 1 / 29 | — | — |

## Post row

| format | bytes | bytes (cover = None) | encode ns | encode allocs (n / B) | decode ns | decode allocs (n / B) | borrowed decode ns | borrowed allocs (n / B) |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| tyx | 326 | 275 | 60.4 | 0 / 0 | 303 | 9 / 384 | 105 | 1 / 80 |
| tyx (encode appended into pre-sized `Writer`) | 326 | | 61.8 | 0 / 0 | | | | |
| bincode 2 (serde) | 333 | 282 | 56.4 | 0 / 0 | 316 | 9 / 384 | 117 | 1 / 80 |
| postcard | 325 | 276 | 80.2 | 0 / 0 | 290 | 9 / 384 | 122 | 1 / 80 |
| bitcode (derive) | 330 | 281 | 116 | 0 / 0 | 321 | 12 / 408 | — | — |
| bitcode (serde) | 332 | 283 | 1297 | 37 / 4158 | 975 | 22 / 2864 | — | — |
| rkyv | 400 | 360 | 54.3 | 0 / 0 | 251 | 9 / 384 | — | — |
| protobuf (prost) | 340 | 286 | 63.0 | 0 / 0 | 488 | 14 / 616 | — | — |
| flatbuffers | 480 | 400 | 129 | 1 / 20 | 410 | 9 / 384 | — | — |
| capnp | 456 | 392 | 108 | 1 / 64 | 450 | 10 / 552 | — | — |
| capnp packed | 382 | 324 | 348 | 2 / 8256 | 630 | 12 / 1016 | — | — |
| pot | 421 | 352 | 316 | 3 / 672 | 692 | 13 / 1248 | — | — |
| msgpack (rmp, array) | 348 | 296 | 74.7 | 0 / 0 | 334 | 9 / 384 | — | — |
| msgpack (rmp, named) | 419 | 350 | 79.2 | 0 / 0 | 419 | 9 / 384 | — | — |
| cbor (ciborium) | 417 | 348 | 157 | 0 / 0 | 977 | 9 / 384 | — | — |
| json (serde_json) | 515 | 439 | 286 | 0 / 0 | 616 | 10 / 552 | — | — |

## Document row

| format | bytes | bytes (parent/seq/fetched_at/changed_at = None) | encode ns | encode allocs (n / B) | decode ns | decode allocs (n / B) | borrowed decode ns | borrowed allocs (n / B) |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| tyx | 230 | 197 | 49.6 | 0 / 0 | 171 | 4 / 238 | 101 | 1 / 64 |
| tyx (encode appended into pre-sized `Writer`) | 230 | | 56.0 | 0 / 0 | | | | |
| bincode 2 (serde) | 244 | 204 | 42.5 | 0 / 0 | 215 | 4 / 238 | 148 | 1 / 64 |
| postcard | 231 | 198 | 61.8 | 0 / 0 | 183 | 4 / 238 | 127 | 1 / 64 |
| bitcode (derive) | 243 | 203 | 80.0 | 0 / 0 | 305 | 12 / 310 | — | — |
| bitcode (serde) | 245 | 204 | 1313 | 38 / 4909 | 1787 | 27 / 4662 | — | — |
| rkyv | 304 | 304 | 34.9 | 0 / 0 | 145 | 4 / 238 | — | — |
| protobuf (prost) | 241 | 204 | 59.8 | 0 / 0 | 286 | 8 / 350 | — | — |
| flatbuffers | 352 | 304 | 102 | 1 / 8 | 246 | 4 / 318 | — | — |
| capnp | 312 | 312 | 73.9 | 1 / 64 | 193 | 4 / 318 | — | — |
| capnp packed | 267 | 231 | 273 | 2 / 8256 | 336 | 6 / 638 | — | — |
| pot | 344 | 312 | 333 | 3 / 672 | 667 | 8 / 1102 | — | — |
| msgpack (rmp, array) | 262 | 225 | 65.7 | 0 / 0 | 281 | 4 / 238 | — | — |
| msgpack (rmp, named) | 354 | 317 | 71.8 | 0 / 0 | 338 | 4 / 238 | — | — |
| cbor (ciborium) | 352 | 316 | 145 | 0 / 0 | 810 | 4 / 238 | — | — |
| json (serde_json) | 463 | 408 | 281 | 0 / 0 | 664 | 7 / 431 | — | — |

## Post: borrowed / zero-copy access

Full view = obtain the borrowed/zero-copy view and read every field. Index filter = read `state` and `changed` only (what a `by_feed (state, changed)` index maintenance or filter needs). Formats not listed need a full owned decode (see the Post table).

| format / access path | full view ns | full view allocs (n / B) | index filter ns | index filter allocs (n / B) |
|---|--:|--:|--:|--:|
| tyx borrowed (`PostRef<'_>`, validates all UTF-8) | 122 | 1 / 80 | 114 | 1 / 80 |
| tyx `Reader` skipping to the fields (hand-written) | — | — | 12.4 | 0 / 0 |
| postcard borrowed | 133 | 1 / 80 | 127 | 1 / 80 |
| bincode 2 borrowed | 127 | 1 / 80 | 123 | 1 / 80 |
| rkyv `access` (validated) | 32.6 | 0 / 0 | 28.8 | 0 / 0 |
| rkyv `access_unchecked` | 4.0 | 0 / 0 | 0.6 | 0 / 0 |
| flatbuffers `root` (verified) | 199 | 0 / 0 | 180 | 0 / 0 |
| flatbuffers `root_unchecked` | 11.7 | 0 / 0 | 1.6 | 0 / 0 |
| capnp reader (lazy, bounds-checked) | 149 | 0 / 0 | 13.3 | 0 / 0 |

## Batch: `Vec<Post>` × 10000 (varied lengths, ~50% `cover`, 0–8 tags)

`whole` = the Vec encoded as one message; `Σ rows` = each Post encoded alone (what a row store keeps); zstd level 3. Peak heap = max live heap above the baseline while decoding the whole batch (includes the result).

| format | whole B | whole zstd B | Σ rows B | Σ rows, concatenated, zstd B | encode ms | decode ms | decode allocs (n / B) | peak heap during decode B | borrowed decode ms |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| tyx | 3051943 | 1192800 | 3051941 | 1192799 | 0.99 | 3.91 | 73672 / 5191702 | 5191702 | 1.63 |
| tyx (encode appended into pre-sized `Writer`) | | | | | 1.19 | | | | |
| bincode 2 (serde) | 3116975 | 1202594 | 3116972 | 1202599 | 0.94 | 3.55 | 73673 / 6576998 | 5528566 | 1.74 |
| postcard | 3051695 | 1178775 | 3051693 | 1178752 | 1.06 | 3.90 | 73673 / 6576998 | 5528566 | 1.77 |
| bitcode (derive) | 3046321 | 1030825 | 3113862 | 1189167 | 0.38 | 2.52 | 73676 / 5061734 | 5061734 | — |
| bitcode (serde) | 3048827 | 1036776 | 3133862 | 1201937 | 1.08 | 3.49 | 93690 / 6949558 | 5580774 | — |
| rkyv | 3723800 | 1338564 | 3743688 | 1334463 | 0.93 | 3.45 | 73672 / 5191702 | 5191702 | — |
| protobuf (prost) | 3189477 | 1239552 | 3159557 | 1215836 | 1.15 | 5.12 | 118137 / 10603474 | 6973106 | — |
| flatbuffers | 3900392 | 1424552 | 4209808 | 1386157 | 1.90 | 5.28 | 73672 / 5349142 | 5349142 | — |
| capnp | 4308688 | 1417462 | 4198592 | 1387862 | 1.18 | 5.00 | 78137 / 9943366 | 6632998 | — |
| capnp packed | 3622408 | 1517671 | 3506133 | 1474837 | 3.17 | 6.28 | 78140 / 14252110 | 10941726 | — |
| pot | 3310362 | 1265898 | 3897400 | 1266225 | 1.59 | 5.98 | 73677 / 6577862 | 5529142 | — |
| msgpack (rmp, array) | 3234832 | 1215739 | 3234829 | 1215703 | 1.15 | 4.30 | 73673 / 6576998 | 5528566 | — |
| msgpack (rmp, named) | 3858336 | 1257521 | 3858333 | 1257498 | 1.36 | 4.87 | 73673 / 6576998 | 5528566 | — |
| cbor (ciborium) | 3851262 | 1256149 | 3851259 | 1256166 | 1.82 | 8.98 | 73673 / 6597106 | 5548674 | — |
| json (serde_json) | 4780755 | 1370630 | 4770754 | 1370075 | 3.11 | 7.12 | 78137 / 9943366 | 6632998 | — |

## tyx frame-header widening

`Writer::begin` reserves 1 header byte; `Writer::end` widens the header once the body exceeds 63 B, shifting the frame body by the extra bytes. `ideal` writes byte-identical output with the length known up front (no shift). Pre-sized = appended into a large pre-allocated `Writer`, so no allocation is measured; `to_vec` is the public one-shot API.

| value | body B | header B | `to_vec` ns | pre-sized ns | ideal pre-sized ns | widening overhead ns |
|---|--:|--:|--:|--:|--:|--:|
| `struct { s: String }`, len 40 | 41 | 1 | 21.5 | 4.3 | 4.9 | -0.5 |
| `struct { s: String }`, len 56 | 57 | 1 | 22.1 | 4.9 | 5.4 | -0.5 |
| `struct { s: String }`, len 60 | 61 | 1 | 23.5 | 6.4 | 6.9 | -0.5 |
| `struct { s: String }`, len 62 | 63 | 1 | 24.1 | 6.9 | 7.5 | -0.5 |
| `struct { s: String }`, len 63 | 64 | 2 | 29.9 | 13.0 | 7.7 | 5.3 |
| `struct { s: String }`, len 64 | 65 | 2 | 25.7 | 8.9 | 3.8 | 5.0 |
| `struct { s: String }`, len 70 | 71 | 2 | 25.7 | 9.9 | 4.2 | 5.7 |
| `struct { s: String }`, len 126 | 127 | 2 | 26.0 | 10.1 | 4.8 | 5.3 |
| `struct { s: String }`, len 127 | 128 | 2 | 26.2 | 10.7 | 5.0 | 5.6 |
| `struct { s: String }`, len 200 | 202 | 2 | 29.8 | 13.4 | 6.9 | 6.5 |
| `struct { s: String }`, len 330 | 332 | 2 | 68.9 | 16.9 | 9.5 | 7.3 |
| `struct { s: String }`, len 1000 | 1002 | 2 | 129 | 33.6 | 21.2 | 12.5 |
| `struct { s: String }`, len 4000 | 4002 | 2 | 219 | 117 | 70.5 | 46.6 |
| `struct { s: String }`, len 8189 | 8191 | 2 | 370 | 248 | 137 | 111 |
| `struct { s: String }`, len 8190 | 8192 | 3 | 402 | 262 | 141 | 120 |
| `struct { s: String }`, len 16000 | 16002 | 3 | 643 | 567 | 243 | 324 |
| Post, body < 64 B | 61 | 1 | 38.0 | 19.6 | — | — |
| Post, body ≥ 64 B (+8 B title) | 69 | 2 | 41.7 | 25.6 | — | — |
| Post (dataset row) | 324 | 2 | 117 | 66.1 | — | — |

## Schema evolution

Probe: a newer writer appends `summary: Option<String>` to `Post` and `alt: Option<String>` to the nested `Image` (both set); the old `Post` type decodes it.

| format | old reader skips appended fields (probed) | preserves unknown fields on re-encode | cost of an absent optional field |
|---|---|---|---|
| tyx | yes (decoded equal) | yes with a `#[tyx(unknown)]` field: it keeps the unknown mask bits and frame tail and writes them back; without one the decoder skips them | 0 B when absent; one mask varint per struct (1 B for ≤7 optional fields), omitted when no optional field is set |
| protobuf (prost) | yes (decoded equal) | no: prost drops unknown fields | 0 B when absent |
| bincode 2 (serde) | **no: silently decodes a wrong value** | n/a (positional, unframed) | 1 B tag |
| postcard | **no: silently decodes a wrong value** | n/a (positional, unframed) | 1 B tag |
| bitcode (serde) | no: error `bitcode error` | n/a | ~1 bit tag (bit-packed); same for bitcode derive |
| pot | yes (decoded equal) | no (serde) | 1 B none marker + field-name symbol |
| msgpack (rmp, array) | no: error `array had incorrect length, expected 3` | no (serde) | 1 B nil |
| msgpack (rmp, named) | yes (decoded equal) | no (serde) | 1 B nil + key string |
| cbor (ciborium) | yes (decoded equal) | no (serde) | 1 B null + key string |
| json (serde_json) | yes (decoded equal) | no (serde) | `null` + key |
| rkyv | not probed: no (fixed archived layout; a changed type fails validation or is misread) | no | inline `Option` tag + full `T` slot (absent costs the same as present) |
| flatbuffers | not probed: yes by design (tables, vtables) | no via generated API (bytes can be passed through) | 0 B data; a vtable slot (2 B) only if a later field is set |
| capnp | not probed: yes by design | yes: copying a struct reader into a builder keeps unknown fields | scalar slot always reserved in the data section; here `union` adds a 2 B discriminant; absent pointer = 8 B null |

Total runtime: 20.0 s
