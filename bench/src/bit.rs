use crate::model::{self, DocumentKind, DocumentState, PostState, Role, Source, from_millis, to_millis};
use uuid::Uuid;

#[derive(bitcode::Encode, bitcode::Decode)]
pub struct User {
    id: Uuid,
    email: String,
    role: Role,
    created: i64,
    last_seen: Option<i64>,
}

#[derive(bitcode::Encode, bitcode::Decode)]
pub struct Image {
    key: String,
    width: u16,
    height: u16,
}

#[derive(bitcode::Encode, bitcode::Decode)]
pub struct Post {
    id: Uuid,
    author: Uuid,
    slug: String,
    title: String,
    state: PostState,
    created: i64,
    changed: i64,
    cover: Option<Image>,
    tags: Vec<String>,
}

#[derive(bitcode::Encode, bitcode::Decode)]
pub struct Etag {
    value: String,
    seen_at: i64,
}

#[derive(bitcode::Encode, bitcode::Decode)]
pub struct Document {
    id: Uuid,
    source: Source,
    kind: DocumentKind,
    key: String,
    parent: Option<Uuid>,
    seq: Option<u64>,
    state: DocumentState,
    fetched_at: Option<i64>,
    changed_at: Option<i64>,
    etags: Vec<Etag>,
}

pub trait Bit: Sized {
    type Native: bitcode::Encode + bitcode::DecodeOwned;
    fn to_bit(&self) -> Self::Native;
    fn from_bit(n: Self::Native) -> Self;
}

impl Bit for model::User {
    type Native = User;

    fn to_bit(&self) -> User {
        User { id: self.id, email: self.email.clone(), role: self.role, created: to_millis(self.created), last_seen: self.last_seen.map(to_millis) }
    }

    fn from_bit(n: User) -> Self {
        Self { id: n.id, email: n.email, role: n.role, created: from_millis(n.created), last_seen: n.last_seen.map(from_millis) }
    }
}

impl Bit for model::Post {
    type Native = Post;

    fn to_bit(&self) -> Post {
        Post {
            id: self.id,
            author: self.author,
            slug: self.slug.clone(),
            title: self.title.clone(),
            state: self.state,
            created: to_millis(self.created),
            changed: to_millis(self.changed),
            cover: self.cover.as_ref().map(|c| Image { key: c.key.clone(), width: c.width, height: c.height }),
            tags: self.tags.clone(),
        }
    }

    fn from_bit(n: Post) -> Self {
        Self {
            id: n.id,
            author: n.author,
            slug: n.slug,
            title: n.title,
            state: n.state,
            created: from_millis(n.created),
            changed: from_millis(n.changed),
            cover: n.cover.map(|c| model::Image { key: c.key, width: c.width, height: c.height }),
            tags: n.tags,
        }
    }
}

impl Bit for model::Document {
    type Native = Document;

    fn to_bit(&self) -> Document {
        Document {
            id: self.id,
            source: self.source,
            kind: self.kind,
            key: self.key.clone(),
            parent: self.parent,
            seq: self.seq,
            state: self.state,
            fetched_at: self.fetched_at.map(to_millis),
            changed_at: self.changed_at.map(to_millis),
            etags: self.etags.iter().map(|e| Etag { value: e.value.clone(), seen_at: to_millis(e.seen_at) }).collect(),
        }
    }

    fn from_bit(n: Document) -> Self {
        Self {
            id: n.id,
            source: n.source,
            kind: n.kind,
            key: n.key,
            parent: n.parent,
            seq: n.seq,
            state: n.state,
            fetched_at: n.fetched_at.map(from_millis),
            changed_at: n.changed_at.map(from_millis),
            etags: n.etags.into_iter().map(|e| model::Etag { value: e.value, seen_at: from_millis(e.seen_at) }).collect(),
        }
    }
}

impl Bit for Vec<model::Post> {
    type Native = Vec<Post>;

    fn to_bit(&self) -> Vec<Post> {
        self.iter().map(Bit::to_bit).collect()
    }

    fn from_bit(n: Vec<Post>) -> Self {
        n.into_iter().map(model::Post::from_bit).collect()
    }
}
