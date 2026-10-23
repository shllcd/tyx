use crate::capn;
use crate::flat::{self, fb};
use crate::formats::{self, Dataset, Visit};
use crate::meter::{self, Op, op};
use crate::model::{ArchivedPost, ArchivedPostState, Blob, Post, PostRef, PostState, PostV2, to_millis};
use crate::proto::{self, Proto};
use std::hint::black_box;
use tyx::{Encode, Writer};

fn ns(x: f64) -> String {
    if x < 100.0 { format!("{x:.1}") } else { format!("{x:.0}") }
}

fn ms(x: f64) -> String {
    format!("{:.2}", x / 1e6)
}

fn allocs(op: &Op) -> String {
    format!("{} / {}", op.allocs, op.alloc_bytes)
}

struct Line {
    format: &'static str,
    bytes: Vec<u8>,
    size_none: usize,
    enc: Op,
    dec: Op,
    borrowed: Option<Op>,
}

struct RowBench<'a, T> {
    value: &'a T,
    none: &'a T,
    lines: Vec<Line>,
}

impl<T: Dataset> Visit<T> for RowBench<'_, T> {
    fn native<M>(
        &mut self,
        format: &'static str,
        to_native: impl Fn(&T) -> M,
        from_native: impl Fn(M) -> T,
        mut enc: impl FnMut(&M, &mut Vec<u8>),
        dec: impl Fn(&[u8]) -> M,
    ) {
        let native = to_native(self.value);
        let mut buf = Vec::new();
        enc(&native, &mut buf);
        let bytes = buf.clone();
        assert_eq!(bytes.as_ptr() as usize % 16, 0);
        assert_eq!(&from_native(dec(&bytes)), self.value, "{format}: round trip");
        let none_native = to_native(self.none);
        let mut none_bytes = Vec::new();
        enc(&none_native, &mut none_bytes);
        assert_eq!(&from_native(dec(&none_bytes)), self.none, "{format}: round trip with None");
        let enc_op = op(|| {
            buf.clear();
            enc(black_box(&native), &mut buf);
            black_box(&buf);
        });
        let dec_op = op(|| dec(black_box(&bytes)));
        self.lines.push(Line { format, bytes, size_none: none_bytes.len(), enc: enc_op, dec: dec_op, borrowed: None });
    }

    fn borrowed(&mut self, dec: impl Fn(&[u8]), reencode: impl Fn(&[u8]) -> Vec<u8>) {
        let line = self.lines.last_mut().expect("borrowed follows a format");
        assert_eq!(reencode(&line.bytes), line.bytes, "{}: borrowed round trip", line.format);
        let bytes = &line.bytes;
        line.borrowed = Some(op(|| dec(black_box(bytes))));
    }
}

const PRESIZED_CHUNK: usize = 1024;

fn presized(item_len: usize, mut encode: impl FnMut(&mut Writer)) -> Op {
    let capacity = PRESIZED_CHUNK * (item_len + 16);
    let mut w = Writer::with_capacity(capacity);
    let mut count = 0;
    op(|| {
        if count == PRESIZED_CHUNK {
            w = Writer::with_capacity(capacity);
            count = 0;
        }
        encode(black_box(&mut w));
        count += 1;
    })
}

pub fn rows<T: Dataset>(name: &str, value: &T, none: &T, none_label: &str) {
    let mut bench = RowBench { value, none, lines: Vec::new() };
    formats::for_each(&mut bench);
    let tyx_len = bench.lines[0].bytes.len();
    let tyx_presized = presized(tyx_len, |w| value.encode(w));
    println!("\n## {name} row\n");
    println!(
        "| format | bytes | bytes ({none_label}) | encode ns | encode allocs (n / B) | decode ns | decode allocs (n / B) | borrowed decode ns | borrowed allocs (n / B) |"
    );
    println!("|---|--:|--:|--:|--:|--:|--:|--:|--:|");
    for l in &bench.lines {
        let (bns, ballocs) = l.borrowed.as_ref().map_or(("—".into(), "—".into()), |b| (ns(b.ns), allocs(b)));
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            l.format,
            l.bytes.len(),
            l.size_none,
            ns(l.enc.ns),
            allocs(&l.enc),
            ns(l.dec.ns),
            allocs(&l.dec),
            bns,
            ballocs
        );
        if l.format == "tyx" {
            println!(
                "| tyx (encode appended into pre-sized `Writer`) | {tyx_len} | | {} | {} | | | | |",
                ns(tyx_presized.ns),
                allocs(&tyx_presized)
            );
        }
    }
}

fn touch_ref(p: &PostRef) -> u64 {
    let cover = p.cover.as_ref().map_or(0, |c| (c.key.len() + usize::from(c.width) + usize::from(c.height)) as u64);
    u64::from(p.id.as_bytes()[0])
        + u64::from(p.author.as_bytes()[0])
        + p.slug.len() as u64
        + p.title.len() as u64
        + p.state as u64
        + to_millis(p.created) as u64
        + to_millis(p.changed) as u64
        + cover
        + p.tags.iter().map(|t| t.len() as u64).sum::<u64>()
}

fn touch_rkyv(p: &ArchivedPost) -> u64 {
    let cover = p.cover.as_ref().map_or(0, |c| c.key.len() as u64 + u64::from(c.width.to_native()) + u64::from(c.height.to_native()));
    u64::from(p.id.as_bytes()[0])
        + u64::from(p.author.as_bytes()[0])
        + p.slug.len() as u64
        + p.title.len() as u64
        + p.state as u64
        + p.created.to_native() as u64
        + p.changed.to_native() as u64
        + cover
        + p.tags.iter().map(|t| t.len() as u64).sum::<u64>()
}

fn tyx_feed_key(bytes: &[u8]) -> Result<(bool, i64), tyx::Error> {
    let (mut body, _mask) = tyx::Reader::new(bytes).frame()?;
    let _id_and_author = body.take(32)?;
    let _slug = body.len_prefixed()?;
    let _title = body.len_prefixed()?;
    let (state, _) = body.tag()?;
    let _created = body.zigzag()?;
    let changed = body.zigzag()?;
    Ok((state == PostState::Published as u32, changed))
}

fn rkyv_feed_key(p: &ArchivedPost) -> (bool, i64) {
    (p.state == ArchivedPostState::Published, p.changed.to_native())
}

fn ref_feed_key(p: &PostRef) -> (bool, i64) {
    (p.state == PostState::Published, to_millis(p.changed))
}

fn fb_feed_key(p: fb::Post) -> (bool, i64) {
    (p.state() == fb::PostState::Published, p.changed())
}

fn capnp_post(bytes: &[u8]) -> capnp::message::Reader<capnp::serialize::NoAllocSliceSegments<'_>> {
    let mut slice = bytes;
    capnp::serialize::read_message_from_flat_slice_no_alloc(&mut slice, capn::OPTIONS).expect("capnp")
}

fn capnp_feed_key(bytes: &[u8]) -> capnp::Result<(bool, i64)> {
    let msg = capnp_post(bytes);
    let p = msg.get_root::<crate::rows_capnp::post::Reader>()?;
    Ok((p.get_state()? == crate::rows_capnp::PostState::Published, p.get_changed()))
}

fn capnp_touch(bytes: &[u8]) -> capnp::Result<u64> {
    let msg = capnp_post(bytes);
    capn::touch(msg.get_root()?)
}

pub fn post_access(post: &Post) {
    let expect_key = (post.state == PostState::Published, to_millis(post.changed));
    let expect_touch = touch_ref(&tyx::from_slice::<PostRef>(&tyx::to_vec(post)).expect("tyx"));

    let tyx = tyx::to_vec(post);
    let postcard = postcard::to_allocvec(post).expect("postcard");
    let bincode = bincode::serde::encode_to_vec(post, bincode::config::standard()).expect("bincode");
    let rkyv = post.rkyv_encode(Vec::new());
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    flat::Flat::fb_build(post, &mut fbb);
    let fbuf = fbb.finished_data().to_vec();
    let mut msg = capnp::message::Builder::new_default();
    capn::Capn::capnp_build(post, &mut msg);
    let cbuf = capnp::serialize::write_message_to_words(&msg);

    fn tyx_ref(b: &[u8]) -> PostRef<'_> {
        tyx::from_slice(b).expect("tyx")
    }
    fn postcard_ref(b: &[u8]) -> PostRef<'_> {
        postcard::from_bytes(b).expect("postcard")
    }
    fn bincode_ref(b: &[u8]) -> PostRef<'_> {
        bincode::serde::borrow_decode_from_slice(b, bincode::config::standard()).expect("bincode").0
    }
    fn rkyv_checked(b: &[u8]) -> &ArchivedPost {
        rkyv::access::<ArchivedPost, rkyv::rancor::Error>(b).expect("rkyv")
    }
    fn rkyv_unchecked(b: &[u8]) -> &ArchivedPost {
        // SAFETY: only called on bytes produced by `rkyv_encode` for a `Post` in this process.
        unsafe { rkyv::access_unchecked::<ArchivedPost>(b) }
    }
    fn fb_checked(b: &[u8]) -> fb::Post<'_> {
        flatbuffers::root::<fb::Post>(b).expect("flatbuffers")
    }
    fn fb_unchecked(b: &[u8]) -> fb::Post<'_> {
        // SAFETY: only called on a finished `fb.Post` buffer built in this process.
        unsafe { flatbuffers::root_unchecked::<fb::Post>(b) }
    }

    let cases: Vec<(&str, Option<Op>, Op)> = vec![
        (
            "tyx borrowed (`PostRef<'_>`, validates all UTF-8)",
            Some(op(|| touch_ref(&tyx_ref(black_box(&tyx))))),
            op(|| ref_feed_key(&tyx_ref(black_box(&tyx)))),
        ),
        ("tyx `Reader` skipping to the fields (hand-written)", None, op(|| tyx_feed_key(black_box(&tyx)).expect("tyx"))),
        (
            "postcard borrowed",
            Some(op(|| touch_ref(&postcard_ref(black_box(&postcard))))),
            op(|| ref_feed_key(&postcard_ref(black_box(&postcard)))),
        ),
        (
            "bincode 2 borrowed",
            Some(op(|| touch_ref(&bincode_ref(black_box(&bincode))))),
            op(|| ref_feed_key(&bincode_ref(black_box(&bincode)))),
        ),
        (
            "rkyv `access` (validated)",
            Some(op(|| touch_rkyv(rkyv_checked(black_box(&rkyv))))),
            op(|| rkyv_feed_key(rkyv_checked(black_box(&rkyv)))),
        ),
        (
            "rkyv `access_unchecked`",
            Some(op(|| touch_rkyv(rkyv_unchecked(black_box(&rkyv))))),
            op(|| rkyv_feed_key(rkyv_unchecked(black_box(&rkyv)))),
        ),
        (
            "flatbuffers `root` (verified)",
            Some(op(|| flat::touch(fb_checked(black_box(&fbuf))))),
            op(|| fb_feed_key(fb_checked(black_box(&fbuf)))),
        ),
        (
            "flatbuffers `root_unchecked`",
            Some(op(|| flat::touch(fb_unchecked(black_box(&fbuf))))),
            op(|| fb_feed_key(fb_unchecked(black_box(&fbuf)))),
        ),
        (
            "capnp reader (lazy, bounds-checked)",
            Some(op(|| capnp_touch(black_box(&cbuf)).expect("capnp"))),
            op(|| capnp_feed_key(black_box(&cbuf)).expect("capnp")),
        ),
    ];

    assert_eq!(tyx_feed_key(&tyx).expect("tyx"), expect_key);
    assert_eq!(ref_feed_key(&postcard_ref(&postcard)), expect_key);
    assert_eq!(ref_feed_key(&bincode_ref(&bincode)), expect_key);
    assert_eq!(rkyv_feed_key(rkyv_checked(&rkyv)), expect_key);
    assert_eq!(fb_feed_key(fb_checked(&fbuf)), expect_key);
    assert_eq!(capnp_feed_key(&cbuf).expect("capnp"), expect_key);
    assert_eq!(touch_ref(&postcard_ref(&postcard)), expect_touch);
    assert_eq!(touch_rkyv(rkyv_checked(&rkyv)), expect_touch);
    assert_eq!(flat::touch(fb_checked(&fbuf)), expect_touch);

    println!("\n## Post: borrowed / zero-copy access\n");
    println!(
        "Full view = obtain the borrowed/zero-copy view and read every field. Index filter = read `state` and `changed` only (what a `by_feed (state, changed)` index maintenance or filter needs). Formats not listed need a full owned decode (see the Post table).\n"
    );
    println!("| format / access path | full view ns | full view allocs (n / B) | index filter ns | index filter allocs (n / B) |");
    println!("|---|--:|--:|--:|--:|");
    for (name, view, field) in &cases {
        let (vns, vallocs) = view.as_ref().map_or(("—".into(), "—".into()), |v| (ns(v.ns), allocs(v)));
        println!("| {name} | {vns} | {vallocs} | {} | {} |", ns(field.ns), allocs(field));
    }
}

struct BatchLine {
    format: &'static str,
    size: usize,
    zstd: usize,
    enc: Op,
    dec: Op,
    peak: usize,
    borrowed: Option<Op>,
}

struct BatchBench<'a> {
    value: &'a Vec<Post>,
    lines: Vec<BatchLine>,
    bytes: Vec<u8>,
}

impl Visit<Vec<Post>> for BatchBench<'_> {
    fn native<M>(
        &mut self,
        format: &'static str,
        to_native: impl Fn(&Vec<Post>) -> M,
        from_native: impl Fn(M) -> Vec<Post>,
        mut enc: impl FnMut(&M, &mut Vec<u8>),
        dec: impl Fn(&[u8]) -> M,
    ) {
        let native = to_native(self.value);
        let mut buf = Vec::new();
        enc(&native, &mut buf);
        let bytes = buf.clone();
        assert_eq!(bytes.as_ptr() as usize % 16, 0);
        assert!(&from_native(dec(&bytes)) == self.value, "{format}: batch round trip");
        let zstd = zstd::bulk::compress(&bytes, 3).expect("zstd").len();
        let enc_op = op(|| {
            buf.clear();
            enc(black_box(&native), &mut buf);
            black_box(&buf);
        });
        let dec_op = op(|| dec(black_box(&bytes)));
        let peak = meter::peak_heap_above_baseline(|| dec(&bytes));
        self.lines.push(BatchLine { format, size: bytes.len(), zstd, enc: enc_op, dec: dec_op, peak, borrowed: None });
        self.bytes = bytes;
    }

    fn borrowed(&mut self, dec: impl Fn(&[u8]), reencode: impl Fn(&[u8]) -> Vec<u8>) {
        assert!(reencode(&self.bytes) == self.bytes);
        let bytes = &self.bytes;
        self.lines.last_mut().expect("borrowed follows a format").borrowed = Some(op(|| dec(black_box(bytes))));
    }
}

struct RowSizes<'a> {
    posts: &'a [Post],
    sums: Vec<(usize, usize)>,
}

impl Visit<Post> for RowSizes<'_> {
    fn native<M>(
        &mut self,
        _: &'static str,
        to_native: impl Fn(&Post) -> M,
        _: impl Fn(M) -> Post,
        mut enc: impl FnMut(&M, &mut Vec<u8>),
        _: impl Fn(&[u8]) -> M,
    ) {
        let mut buf = Vec::new();
        let mut sum = 0;
        let mut concat = Vec::new();
        for p in self.posts {
            buf.clear();
            enc(&to_native(p), &mut buf);
            sum += buf.len();
            concat.extend_from_slice(&buf);
        }
        self.sums.push((sum, zstd::bulk::compress(&concat, 3).expect("zstd").len()));
    }

    fn borrowed(&mut self, _: impl Fn(&[u8]), _: impl Fn(&[u8]) -> Vec<u8>) {}
}

pub fn batch(posts: &Vec<Post>) {
    let mut bench = BatchBench { value: posts, lines: Vec::new(), bytes: Vec::new() };
    formats::for_each(&mut bench);
    let mut sizes = RowSizes { posts, sums: Vec::new() };
    formats::for_each(&mut sizes);
    let tyx_rows = posts.iter().map(|p| tyx::to_vec(p).len()).sum::<usize>();
    let tyx_presized = presized(tyx_rows, |w| posts.encode(w));

    println!("\n## Batch: `Vec<Post>` × {} (varied lengths, ~50% `cover`, 0–8 tags)\n", posts.len());
    println!(
        "`whole` = the Vec encoded as one message; `Σ rows` = each Post encoded alone (what a row store keeps); zstd level 3. Peak heap = max live heap above the baseline while decoding the whole batch (includes the result).\n"
    );
    println!(
        "| format | whole B | whole zstd B | Σ rows B | Σ rows, concatenated, zstd B | encode ms | decode ms | decode allocs (n / B) | peak heap during decode B | borrowed decode ms |"
    );
    println!("|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|");
    for (l, (sum, sum_zstd)) in bench.lines.iter().zip(&sizes.sums) {
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            l.format,
            l.size,
            l.zstd,
            sum,
            sum_zstd,
            ms(l.enc.ns),
            ms(l.dec.ns),
            allocs(&l.dec),
            l.peak,
            l.borrowed.as_ref().map_or("—".into(), |b| ms(b.ns))
        );
        if l.format == "tyx" {
            println!("| tyx (encode appended into pre-sized `Writer`) | | | | | {} | | | | |", ms(tyx_presized.ns));
        }
    }
}

fn header_width(bytes: &[u8]) -> usize {
    bytes.iter().position(|b| b & 0x80 == 0).expect("header") + 1
}

pub fn widening(post: &Post) {
    println!("\n## tyx frame-header widening\n");
    println!(
        "`Writer::begin` reserves 1 header byte; `Writer::end` splices the header wider (memmove of the frame body) once the body exceeds 63 B. `ideal` writes byte-identical output with the length known up front (no splice). Pre-sized = appended into a large pre-allocated `Writer`, so no allocation is measured; `to_vec` is the public one-shot API.\n"
    );
    println!("| value | body B | header B | `to_vec` ns | pre-sized ns | ideal pre-sized ns | splice overhead ns |");
    println!("|---|--:|--:|--:|--:|--:|--:|");
    for len in [40, 56, 60, 62, 63, 64, 70, 126, 127, 200, 330, 1000, 4000, 8189, 8190, 16000] {
        let blob = Blob { s: "x".repeat(len) };
        let bytes = tyx::to_vec(&blob);
        let mut ideal = Writer::new();
        let body_len = bytes.len() - header_width(&bytes);
        ideal.varint((body_len as u64) << 1);
        ideal.len_prefixed(blob.s.as_bytes());
        assert_eq!(ideal.as_slice(), bytes);
        let to_vec = op(|| tyx::to_vec(black_box(&blob)));
        let pre = presized(bytes.len(), |w| blob.encode(w));
        let ideal_op = presized(bytes.len(), |w| {
            w.varint((body_len as u64) << 1);
            w.len_prefixed(black_box(&blob).s.as_bytes());
        });
        println!(
            "| `struct {{ s: String }}`, len {len} | {body_len} | {} | {} | {} | {} | {} |",
            header_width(&bytes),
            ns(to_vec.ns),
            ns(pre.ns),
            ns(ideal_op.ns),
            ns(pre.ns - ideal_op.ns)
        );
    }
    let small = |title_len: usize| Post { slug: "a".into(), title: "t".repeat(title_len), cover: None, tags: Vec::new(), ..post.clone() };
    for (label, p) in
        [("Post, body < 64 B", small(12)), ("Post, body ≥ 64 B (+8 B title)", small(20)), ("Post (dataset row)", post.clone())]
    {
        let bytes = tyx::to_vec(&p);
        let to_vec = op(|| tyx::to_vec(black_box(&p)));
        let pre = presized(bytes.len(), |w| p.encode(w));
        println!(
            "| {label} | {} | {} | {} | {} | — | — |",
            bytes.len() - header_width(&bytes),
            header_width(&bytes),
            ns(to_vec.ns),
            ns(pre.ns)
        );
    }
}

fn probe<E: std::fmt::Display>(bytes: Vec<u8>, dec: impl Fn(&[u8]) -> Result<Post, E>, expect: &Post) -> String {
    match dec(&bytes) {
        Ok(p) if &p == expect => "yes (decoded equal)".into(),
        Ok(_) => "**no: silently decodes a wrong value**".into(),
        Err(e) => format!("no: error `{}`", e.to_string().chars().take(60).collect::<String>()),
    }
}

pub fn evolution(post: &Post) {
    let v2 = PostV2::from_v1(post);
    let v1_post = post;
    let bc = bincode::config::standard();
    let probes = [
        ("tyx", probe(tyx::to_vec(&v2), |b| tyx::from_slice::<Post>(b), v1_post)),
        ("protobuf (prost)", {
            let bytes = prost::Message::encode_to_vec(&proto::PostV2::from_model(&v2));
            probe(bytes, |b| <proto::Post as prost::Message>::decode(b).map(Post::from_proto), v1_post)
        }),
        (
            "bincode 2 (serde)",
            probe(
                bincode::serde::encode_to_vec(&v2, bc).expect("bincode"),
                |b| bincode::serde::decode_from_slice::<Post, _>(b, bc).map(|r| r.0),
                v1_post,
            ),
        ),
        ("postcard", probe(postcard::to_allocvec(&v2).expect("postcard"), |b| postcard::from_bytes::<Post>(b), v1_post)),
        ("bitcode (serde)", probe(bitcode::serialize(&v2).expect("bitcode"), |b| bitcode::deserialize::<Post>(b), v1_post)),
        ("pot", probe(pot::to_vec(&v2).expect("pot"), |b| pot::from_slice::<Post>(b), v1_post)),
        ("msgpack (rmp, array)", probe(rmp_serde::to_vec(&v2).expect("rmp"), |b| rmp_serde::from_slice::<Post>(b), v1_post)),
        ("msgpack (rmp, named)", probe(rmp_serde::to_vec_named(&v2).expect("rmp"), |b| rmp_serde::from_slice::<Post>(b), v1_post)),
        ("cbor (ciborium)", {
            let mut bytes = Vec::new();
            ciborium::into_writer(&v2, &mut bytes).expect("cbor");
            probe(bytes, |b| ciborium::from_reader::<Post, _>(b), v1_post)
        }),
        ("json (serde_json)", probe(serde_json::to_vec(&v2).expect("json"), |b| serde_json::from_slice::<Post>(b), v1_post)),
    ];
    let qualitative = [
        (
            "tyx",
            "yes with a `#[tyx(unknown)]` field: it keeps the unknown mask bits and frame tail and writes them back; without one the decoder skips them",
            "0 B when absent; one mask varint per struct (1 B for ≤7 optional fields), omitted when no optional field is set",
        ),
        ("protobuf (prost)", "no: prost drops unknown fields", "0 B when absent"),
        ("bincode 2 (serde)", "n/a (positional, unframed)", "1 B tag"),
        ("postcard", "n/a (positional, unframed)", "1 B tag"),
        ("bitcode (serde)", "n/a", "~1 bit tag (bit-packed); same for bitcode derive"),
        ("pot", "no (serde)", "1 B none marker + field-name symbol"),
        ("msgpack (rmp, array)", "no (serde)", "1 B nil"),
        ("msgpack (rmp, named)", "no (serde)", "1 B nil + key string"),
        ("cbor (ciborium)", "no (serde)", "1 B null + key string"),
        ("json (serde_json)", "no (serde)", "`null` + key"),
    ];
    println!("\n## Schema evolution\n");
    println!(
        "Probe: a newer writer appends `summary: Option<String>` to `Post` and `alt: Option<String>` to the nested `Image` (both set); the old `Post` type decodes it.\n"
    );
    println!(
        "| format | old reader skips appended fields (probed) | preserves unknown fields on re-encode | cost of an absent optional field |"
    );
    println!("|---|---|---|---|");
    for ((name, result), (qname, preserve, cost)) in probes.iter().zip(qualitative) {
        assert_eq!(*name, qname);
        println!("| {name} | {result} | {preserve} | {cost} |");
    }
    println!(
        "| rkyv | not probed: no (fixed archived layout; a changed type fails validation or is misread) | no | inline `Option` tag + full `T` slot (absent costs the same as present) |"
    );
    println!(
        "| flatbuffers | not probed: yes by design (tables, vtables) | no via generated API (bytes can be passed through) | 0 B data; a vtable slot (2 B) only if a later field is set |"
    );
    println!(
        "| capnp | not probed: yes by design | yes: copying a struct reader into a builder keeps unknown fields | scalar slot always reserved in the data section; here `union` adds a 2 B discriminant; absent pointer = 8 B null |"
    );
}
