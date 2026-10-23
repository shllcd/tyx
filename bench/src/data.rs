use crate::model::{Document, DocumentKind, DocumentState, Etag, Image, Post, PostState, Role, Source, User, from_millis};
use time::OffsetDateTime;
use uuid::Uuid;

const WORDS: &[&str] = &[
    "rust",
    "embedded",
    "database",
    "schema",
    "index",
    "query",
    "async",
    "writer",
    "snapshot",
    "replica",
    "frame",
    "varint",
    "migration",
    "bucket",
    "queue",
    "kyoto",
    "garden",
    "lantern",
    "harbor",
    "copper",
    "meadow",
    "signal",
    "orbit",
    "paper",
];

pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    pub fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below((hi - lo + 1) as u64) as usize
    }

    pub fn coin(&mut self) -> bool {
        self.next() & 1 == 1
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }

    pub fn uuid(&mut self) -> Uuid {
        let hi = self.next() << 11 ^ self.next();
        let lo = self.next() << 11 ^ self.next();
        Uuid::from_u64_pair(hi & !0xF000 | 0x7000, lo & !(0b11 << 62) | 1 << 63)
    }

    pub fn time(&mut self) -> OffsetDateTime {
        from_millis(1_600_000_000_000 + self.below(200_000_000_000) as i64)
    }

    pub fn text_between(&mut self, lo: usize, hi: usize, sep: char) -> String {
        let len = self.range(lo, hi);
        self.text(len, sep)
    }

    pub fn text(&mut self, len: usize, sep: char) -> String {
        let mut s = String::with_capacity(len + 12);
        while s.len() < len {
            if !s.is_empty() {
                s.push(sep);
            }
            s.push_str(self.pick(WORDS));
        }
        s.truncate(len);
        s
    }
}

pub fn user(rng: &mut Lcg) -> User {
    User {
        id: rng.uuid(),
        email: format!("{}@{}.example.com", rng.text(10, '.'), rng.pick(WORDS)),
        role: rng.pick(Role::ALL),
        created: rng.time(),
        last_seen: Some(rng.time()),
    }
}

pub fn post(rng: &mut Lcg) -> Post {
    Post {
        id: rng.uuid(),
        author: rng.uuid(),
        slug: rng.text(60, '-'),
        title: rng.text(100, ' '),
        state: PostState::Published,
        created: rng.time(),
        changed: rng.time(),
        cover: Some(Image { key: format!("covers/{}.webp", rng.text(32, '-')), width: 1600, height: 900 }),
        tags: (0..5).map(|_| rng.text(12, '-')).collect(),
    }
}

pub fn varied_post(rng: &mut Lcg) -> Post {
    Post {
        id: rng.uuid(),
        author: rng.uuid(),
        slug: rng.text_between(10, 96, '-'),
        title: rng.text_between(10, 200, ' '),
        state: rng.pick(PostState::ALL),
        created: rng.time(),
        changed: rng.time(),
        cover: rng.coin().then(|| Image {
            key: format!("covers/{}.webp", rng.text_between(4, 50, '-')),
            width: rng.range(64, 4096) as u16,
            height: rng.range(64, 4096) as u16,
        }),
        tags: (0..rng.range(0, 8)).map(|_| rng.text_between(3, 32, '-')).collect(),
    }
}

pub fn document(rng: &mut Lcg) -> Document {
    Document {
        id: rng.uuid(),
        source: Source::Rss,
        kind: DocumentKind::Page,
        key: format!("https://{}.example.org/{}", rng.pick(WORDS), rng.text(60, '/')),
        parent: Some(rng.uuid()),
        seq: Some(rng.below(10_000_000)),
        state: DocumentState::Fetched,
        fetched_at: Some(rng.time()),
        changed_at: Some(rng.time()),
        etags: (0..2).map(|_| Etag { value: format!("W/\"{:032x}\"", rng.next()), seen_at: rng.time() }).collect(),
    }
}

pub fn batch(rng: &mut Lcg, count: usize) -> Vec<Post> {
    (0..count).map(|_| varied_post(rng)).collect()
}
