# tyx serialization benchmark: results

Command:

```sh
cargo xtask bench
```

Machine: Apple M3, macOS 26.6.2. Toolchain: rustc 1.98.1 (release profile: `lto = "fat"`, `codegen-units = 1`). External tools: flatc 25.12.19, capnp 1.2.0.

Crates (from `Cargo.lock`): tyx 0.1.0 (path, features `time`, `uuid`), bincode 2.0.1, bitcode 0.6.9, capnp 0.27.2, capnpc 0.27.0, ciborium 0.2.2, flatbuffers 25.12.19, pot 3.0.1, postcard 1.1.3, prost 0.14.4, rkyv 0.8.18, rmp-serde 1.3.1, serde 1.0.229, serde_json 1.0.154, time 0.3.55, uuid 1.28.0, zstd 0.14.3.

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
| tyx | 61 | 54 | 17.2 | 0 / 0 | 65.6 | 1 / 29 | 35.3 | 0 / 0 |
| tyx (encode appended into pre-sized `Writer`) | 61 | | 16.9 | 0 / 0 | | | | |
| bincode 2 (serde) | 67 | 58 | 19.7 | 0 / 0 | 63.8 | 1 / 29 | 42.4 | 0 / 0 |
| postcard | 61 | 55 | 28.9 | 0 / 0 | 68.9 | 1 / 29 | 40.5 | 0 / 0 |
| bitcode (derive) | 67 | 58 | 44.7 | 0 / 0 | 88.7 | 3 / 45 | — | — |
| bitcode (serde) | 68 | 59 | 590 | 16 / 1347 | 579 | 8 / 1341 | — | — |
| rkyv | 88 | 88 | 19.2 | 0 / 0 | 52.9 | 1 / 29 | — | — |
| protobuf (prost) | 65 | 58 | 18.0 | 0 / 0 | 108 | 3 / 61 | — | — |
| flatbuffers | 104 | 96 | 36.5 | 0 / 0 | 86.1 | 1 / 29 | — | — |
| capnp | 96 | 96 | 48.7 | 1 / 64 | 68.4 | 1 / 29 | — | — |
| capnp packed | 75 | 69 | 208 | 2 / 8256 | 160 | 3 / 133 | — | — |
| pot | 108 | 102 | 135 | 2 / 288 | 300 | 4 / 509 | — | — |
| msgpack (rmp, array) | 73 | 65 | 25.7 | 0 / 0 | 78.6 | 1 / 29 | — | — |
| msgpack (rmp, named) | 105 | 97 | 28.9 | 0 / 0 | 112 | 1 / 29 | — | — |
| cbor (ciborium) | 105 | 97 | 64.5 | 0 / 0 | 292 | 1 / 29 | — | — |
| json (serde_json) | 150 | 141 | 104 | 0 / 0 | 190 | 1 / 29 | — | — |

## Post row

| format | bytes | bytes (cover = None) | encode ns | encode allocs (n / B) | decode ns | decode allocs (n / B) | borrowed decode ns | borrowed allocs (n / B) |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| tyx | 326 | 275 | 57.6 | 0 / 0 | 310 | 9 / 384 | 118 | 1 / 80 |
| tyx (encode appended into pre-sized `Writer`) | 326 | | 59.1 | 0 / 0 | | | | |
| bincode 2 (serde) | 333 | 282 | 64.7 | 0 / 0 | 341 | 9 / 384 | 124 | 1 / 80 |
| postcard | 325 | 276 | 84.9 | 0 / 0 | 312 | 9 / 384 | 124 | 1 / 80 |
| bitcode (derive) | 330 | 281 | 109 | 0 / 0 | 334 | 12 / 408 | — | — |
| bitcode (serde) | 332 | 283 | 1398 | 37 / 4158 | 1070 | 22 / 2864 | — | — |
| rkyv | 400 | 360 | 58.8 | 0 / 0 | 262 | 9 / 384 | — | — |
| protobuf (prost) | 340 | 286 | 68.1 | 0 / 0 | 526 | 14 / 616 | — | — |
| flatbuffers | 480 | 400 | 144 | 1 / 20 | 443 | 9 / 384 | — | — |
| capnp | 456 | 392 | 118 | 1 / 64 | 473 | 10 / 552 | — | — |
| capnp packed | 382 | 324 | 361 | 2 / 8256 | 651 | 12 / 1016 | — | — |
| pot | 421 | 352 | 334 | 3 / 672 | 782 | 13 / 1248 | — | — |
| msgpack (rmp, array) | 348 | 296 | 80.8 | 0 / 0 | 358 | 9 / 384 | — | — |
| msgpack (rmp, named) | 419 | 350 | 84.9 | 0 / 0 | 433 | 9 / 384 | — | — |
| cbor (ciborium) | 417 | 348 | 169 | 0 / 0 | 1049 | 9 / 384 | — | — |
| json (serde_json) | 515 | 439 | 322 | 0 / 0 | 685 | 10 / 552 | — | — |

## Document row

| format | bytes | bytes (parent/seq/fetched_at/changed_at = None) | encode ns | encode allocs (n / B) | decode ns | decode allocs (n / B) | borrowed decode ns | borrowed allocs (n / B) |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| tyx | 230 | 197 | 44.9 | 0 / 0 | 174 | 4 / 238 | 105 | 1 / 64 |
| tyx (encode appended into pre-sized `Writer`) | 230 | | 48.4 | 0 / 0 | | | | |
| bincode 2 (serde) | 244 | 204 | 48.7 | 0 / 0 | 230 | 4 / 238 | 156 | 1 / 64 |
| postcard | 231 | 198 | 67.1 | 0 / 0 | 197 | 4 / 238 | 134 | 1 / 64 |
| bitcode (derive) | 243 | 203 | 88.8 | 0 / 0 | 326 | 12 / 310 | — | — |
| bitcode (serde) | 245 | 204 | 1378 | 38 / 4909 | 1921 | 27 / 4662 | — | — |
| rkyv | 304 | 304 | 40.3 | 0 / 0 | 152 | 4 / 238 | — | — |
| protobuf (prost) | 241 | 204 | 60.3 | 0 / 0 | 289 | 8 / 350 | — | — |
| flatbuffers | 352 | 304 | 108 | 1 / 8 | 274 | 4 / 318 | — | — |
| capnp | 312 | 312 | 86.7 | 1 / 64 | 201 | 4 / 318 | — | — |
| capnp packed | 267 | 231 | 278 | 2 / 8256 | 341 | 6 / 638 | — | — |
| pot | 344 | 312 | 350 | 3 / 672 | 777 | 8 / 1102 | — | — |
| msgpack (rmp, array) | 262 | 225 | 67.9 | 0 / 0 | 285 | 4 / 238 | — | — |
| msgpack (rmp, named) | 354 | 317 | 77.9 | 0 / 0 | 356 | 4 / 238 | — | — |
| cbor (ciborium) | 352 | 316 | 157 | 0 / 0 | 821 | 4 / 238 | — | — |
| json (serde_json) | 463 | 408 | 291 | 0 / 0 | 695 | 7 / 431 | — | — |

## Post: borrowed / zero-copy access

Full view = obtain the borrowed/zero-copy view and read every field. Index filter = read `state` and `changed` only (what a `by_feed (state, changed)` index maintenance or filter needs). Formats not listed need a full owned decode (see the Post table).

| format / access path | full view ns | full view allocs (n / B) | index filter ns | index filter allocs (n / B) |
|---|--:|--:|--:|--:|
| tyx borrowed (`PostRef<'_>`, validates all UTF-8) | 124 | 1 / 80 | 121 | 1 / 80 |
| tyx `Reader` skipping to the fields (hand-written) | — | — | 13.7 | 0 / 0 |
| postcard borrowed | 138 | 1 / 80 | 131 | 1 / 80 |
| bincode 2 borrowed | 131 | 1 / 80 | 126 | 1 / 80 |
| rkyv `access` (validated) | 34.0 | 0 / 0 | 30.5 | 0 / 0 |
| rkyv `access_unchecked` | 4.4 | 0 / 0 | 0.6 | 0 / 0 |
| flatbuffers `root` (verified) | 204 | 0 / 0 | 189 | 0 / 0 |
| flatbuffers `root_unchecked` | 13.4 | 0 / 0 | 1.8 | 0 / 0 |
| capnp reader (lazy, bounds-checked) | 147 | 0 / 0 | 14.6 | 0 / 0 |

## Batch: `Vec<Post>` × 10000 (varied lengths, ~50% `cover`, 0–8 tags)

`whole` = the Vec encoded as one message; `Σ rows` = each Post encoded alone (what a row store keeps); zstd level 3. Peak heap = max live heap above the baseline while decoding the whole batch (includes the result).

| format | whole B | whole zstd B | Σ rows B | Σ rows, concatenated, zstd B | encode ms | decode ms | decode allocs (n / B) | peak heap during decode B | borrowed decode ms |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| tyx | 3051943 | 1192800 | 3051941 | 1192799 | 1.03 | 4.13 | 73672 / 5191702 | 5191702 | 1.62 |
| tyx (encode appended into pre-sized `Writer`) | | | | | 1.13 | | | | |
| bincode 2 (serde) | 3116975 | 1202594 | 3116972 | 1202599 | 0.91 | 3.63 | 73673 / 6576998 | 5528566 | 1.81 |
| postcard | 3051695 | 1178775 | 3051693 | 1178752 | 1.13 | 4.07 | 73673 / 6576998 | 5528566 | 1.78 |
| bitcode (derive) | 3046321 | 1030825 | 3113862 | 1189167 | 0.42 | 2.58 | 73676 / 5061734 | 5061734 | — |
| bitcode (serde) | 3048827 | 1036776 | 3133862 | 1201937 | 1.17 | 3.59 | 93690 / 6949558 | 5580774 | — |
| rkyv | 3723800 | 1338564 | 3743688 | 1334463 | 0.90 | 3.45 | 73672 / 5191702 | 5191702 | — |
| protobuf (prost) | 3189477 | 1239552 | 3159557 | 1215836 | 1.13 | 5.26 | 118137 / 10603474 | 6973106 | — |
| flatbuffers | 3900392 | 1424552 | 4209808 | 1386157 | 1.95 | 5.60 | 73672 / 5349142 | 5349142 | — |
| capnp | 4308688 | 1417462 | 4198592 | 1387862 | 1.30 | 5.16 | 78137 / 9943366 | 6632998 | — |
| capnp packed | 3622408 | 1517671 | 3506133 | 1474837 | 3.23 | 6.42 | 78140 / 14252110 | 10941726 | — |
| pot | 3310362 | 1265898 | 3897400 | 1266225 | 1.67 | 6.48 | 73677 / 6577862 | 5529142 | — |
| msgpack (rmp, array) | 3234832 | 1215739 | 3234829 | 1215703 | 1.19 | 4.38 | 73673 / 6576998 | 5528566 | — |
| msgpack (rmp, named) | 3858336 | 1257521 | 3858333 | 1257498 | 1.25 | 4.94 | 73673 / 6576998 | 5528566 | — |
| cbor (ciborium) | 3851262 | 1256149 | 3851259 | 1256166 | 1.86 | 9.34 | 73673 / 6597106 | 5548674 | — |
| json (serde_json) | 4780755 | 1370630 | 4770754 | 1370075 | 3.13 | 7.16 | 78137 / 9943366 | 6632998 | — |

## tyx frame-header widening

`Writer::begin` reserves 1 header byte; `Writer::end` splices the header wider (memmove of the frame body) once the body exceeds 63 B. `ideal` writes byte-identical output with the length known up front (no splice). Pre-sized = appended into a large pre-allocated `Writer`, so no allocation is measured; `to_vec` is the public one-shot API.

| value | body B | header B | `to_vec` ns | pre-sized ns | ideal pre-sized ns | splice overhead ns |
|---|--:|--:|--:|--:|--:|--:|
| `struct { s: String }`, len 40 | 41 | 1 | 24.4 | 4.7 | 5.8 | -1.1 |
| `struct { s: String }`, len 56 | 57 | 1 | 24.6 | 5.2 | 6.1 | -0.9 |
| `struct { s: String }`, len 60 | 61 | 1 | 26.2 | 6.7 | 7.5 | -0.8 |
| `struct { s: String }`, len 62 | 63 | 1 | 25.6 | 7.3 | 8.1 | -0.8 |
| `struct { s: String }`, len 63 | 64 | 2 | 31.5 | 12.7 | 8.9 | 3.8 |
| `struct { s: String }`, len 64 | 65 | 2 | 28.1 | 8.8 | 5.2 | 3.6 |
| `struct { s: String }`, len 70 | 71 | 2 | 27.7 | 10.1 | 6.0 | 4.0 |
| `struct { s: String }`, len 126 | 127 | 2 | 29.4 | 11.0 | 7.7 | 3.3 |
| `struct { s: String }`, len 127 | 128 | 2 | 28.9 | 11.0 | 7.8 | 3.2 |
| `struct { s: String }`, len 200 | 202 | 2 | 32.6 | 15.4 | 10.6 | 4.8 |
| `struct { s: String }`, len 330 | 332 | 2 | 73.9 | 18.2 | 11.9 | 6.3 |
| `struct { s: String }`, len 1000 | 1002 | 2 | 133 | 36.4 | 21.2 | 15.2 |
| `struct { s: String }`, len 4000 | 4002 | 2 | 233 | 120 | 72.4 | 47.7 |
| `struct { s: String }`, len 8189 | 8191 | 2 | 415 | 260 | 250 | 10.5 |
| `struct { s: String }`, len 8190 | 8192 | 3 | 432 | 260 | 256 | 3.8 |
| `struct { s: String }`, len 16000 | 16002 | 3 | 672 | 643 | 499 | 144 |
| Post, body < 64 B | 61 | 1 | 38.3 | 18.8 | — | — |
| Post, body ≥ 64 B (+8 B title) | 69 | 2 | 42.0 | 26.2 | — | — |
| Post (dataset row) | 324 | 2 | 115 | 57.0 | — | — |

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

Total runtime: 20.9 s
