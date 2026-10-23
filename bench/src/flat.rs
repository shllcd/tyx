#[allow(clippy::all, unsafe_op_in_unsafe_fn, mismatched_lifetime_syntaxes, reason = "flatc output")]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/rows_generated.rs"));
}

use crate::model::{self, from_millis, to_millis};
use flatbuffers::{FlatBufferBuilder, ForwardsUOffset, Vector, WIPOffset};
pub use generated::fb;
use uuid::Uuid;

fn uuid(id: &Uuid) -> fb::Uuid {
    fb::Uuid::new(id.as_bytes())
}

fn from_uuid(id: Option<&fb::Uuid>) -> Uuid {
    Uuid::from_bytes(id.expect("uuid present").0)
}

pub trait Flat: Sized {
    fn fb_build(&self, b: &mut FlatBufferBuilder);
    fn fb_read(bytes: &[u8]) -> Self;
}

impl Flat for model::User {
    fn fb_build(&self, b: &mut FlatBufferBuilder) {
        let id = uuid(&self.id);
        let email = b.create_string(&self.email);
        let root = fb::User::create(
            b,
            &fb::UserArgs {
                id: Some(&id),
                email: Some(email),
                role: fb::Role(self.role as u8),
                created: to_millis(self.created),
                last_seen: self.last_seen.map(to_millis),
            },
        );
        b.finish_minimal(root);
    }

    fn fb_read(bytes: &[u8]) -> Self {
        let u = flatbuffers::root::<fb::User>(bytes).expect("valid flatbuffer");
        Self {
            id: from_uuid(u.id()),
            email: u.email().expect("email").to_owned(),
            role: model::Role::ALL[u.role().0 as usize],
            created: from_millis(u.created()),
            last_seen: u.last_seen().map(from_millis),
        }
    }
}

fn build_post<'b>(p: &model::Post, b: &mut FlatBufferBuilder<'b>) -> WIPOffset<fb::Post<'b>> {
    let id = uuid(&p.id);
    let author = uuid(&p.author);
    let slug = b.create_string(&p.slug);
    let title = b.create_string(&p.title);
    let cover = p.cover.as_ref().map(|c| {
        let key = b.create_string(&c.key);
        fb::Image::create(b, &fb::ImageArgs { key: Some(key), width: c.width, height: c.height })
    });
    let tags: Vec<_> = p.tags.iter().map(|t| b.create_string(t)).collect();
    let tags = b.create_vector(&tags);
    fb::Post::create(
        b,
        &fb::PostArgs {
            id: Some(&id),
            author: Some(&author),
            slug: Some(slug),
            title: Some(title),
            state: fb::PostState(p.state as u8),
            created: to_millis(p.created),
            changed: to_millis(p.changed),
            cover,
            tags: Some(tags),
        },
    )
}

fn read_post(p: fb::Post) -> model::Post {
    model::Post {
        id: from_uuid(p.id()),
        author: from_uuid(p.author()),
        slug: p.slug().expect("slug").to_owned(),
        title: p.title().expect("title").to_owned(),
        state: model::PostState::ALL[p.state().0 as usize],
        created: from_millis(p.created()),
        changed: from_millis(p.changed()),
        cover: p.cover().map(|c| model::Image { key: c.key().expect("key").to_owned(), width: c.width(), height: c.height() }),
        tags: p.tags().map(|t| t.iter().map(str::to_owned).collect()).unwrap_or_default(),
    }
}

impl Flat for model::Post {
    fn fb_build(&self, b: &mut FlatBufferBuilder) {
        let root = build_post(self, b);
        b.finish_minimal(root);
    }

    fn fb_read(bytes: &[u8]) -> Self {
        read_post(flatbuffers::root::<fb::Post>(bytes).expect("valid flatbuffer"))
    }
}

impl Flat for model::Document {
    fn fb_build(&self, b: &mut FlatBufferBuilder) {
        let id = uuid(&self.id);
        let parent = self.parent.as_ref().map(uuid);
        let key = b.create_string(&self.key);
        let etags: Vec<_> = self
            .etags
            .iter()
            .map(|e| {
                let value = b.create_string(&e.value);
                fb::Etag::create(b, &fb::EtagArgs { value: Some(value), seen_at: to_millis(e.seen_at) })
            })
            .collect();
        let etags = b.create_vector(&etags);
        let root = fb::Document::create(
            b,
            &fb::DocumentArgs {
                id: Some(&id),
                source: fb::Source(self.source as u8),
                kind: fb::DocumentKind(self.kind as u8),
                key: Some(key),
                parent: parent.as_ref(),
                seq: self.seq,
                state: fb::DocumentState(self.state as u8),
                fetched_at: self.fetched_at.map(to_millis),
                changed_at: self.changed_at.map(to_millis),
                etags: Some(etags),
            },
        );
        b.finish_minimal(root);
    }

    fn fb_read(bytes: &[u8]) -> Self {
        let d = flatbuffers::root::<fb::Document>(bytes).expect("valid flatbuffer");
        Self {
            id: from_uuid(d.id()),
            source: model::Source::ALL[d.source().0 as usize],
            kind: model::DocumentKind::ALL[d.kind().0 as usize],
            key: d.key().expect("key").to_owned(),
            parent: d.parent().map(|p| Uuid::from_bytes(p.0)),
            seq: d.seq(),
            state: model::DocumentState::ALL[d.state().0 as usize],
            fetched_at: d.fetched_at().map(from_millis),
            changed_at: d.changed_at().map(from_millis),
            etags: d
                .etags()
                .map(|v| {
                    v.iter()
                        .map(|e| model::Etag { value: e.value().expect("value").to_owned(), seen_at: from_millis(e.seen_at()) })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

impl Flat for Vec<model::Post> {
    fn fb_build(&self, b: &mut FlatBufferBuilder) {
        let items: Vec<_> = self.iter().map(|p| build_post(p, b)).collect();
        let items = b.create_vector(&items);
        let root = fb::Posts::create(b, &fb::PostsArgs { items: Some(items) });
        b.finish_minimal(root);
    }

    fn fb_read(bytes: &[u8]) -> Self {
        let opts = flatbuffers::VerifierOptions { max_tables: usize::MAX, max_apparent_size: usize::MAX, ..Default::default() };
        let posts = flatbuffers::root_with_opts::<fb::Posts>(&opts, bytes).expect("valid flatbuffer");
        posts.items().map(|v: Vector<ForwardsUOffset<fb::Post>>| v.iter().map(read_post).collect()).unwrap_or_default()
    }
}

pub fn touch(p: fb::Post) -> u64 {
    let cover = p.cover().map_or(0, |c| c.key().map_or(0, str::len) as u64 + u64::from(c.width()) + u64::from(c.height()));
    let tags = p.tags().map_or(0, |t| t.iter().map(|s| s.len() as u64).sum::<u64>());
    u64::from(p.id().map_or(0, |id| id.0[0]))
        + u64::from(p.author().map_or(0, |id| id.0[0]))
        + p.slug().map_or(0, str::len) as u64
        + p.title().map_or(0, str::len) as u64
        + u64::from(p.state().0)
        + p.created() as u64
        + p.changed() as u64
        + cover
        + tags
}
