use crate::model::{self, from_millis, to_millis};
use uuid::Uuid;

#[derive(Clone, PartialEq, prost::Message)]
pub struct User {
    #[prost(bytes = "vec", tag = "1")]
    pub id: Vec<u8>,
    #[prost(string, tag = "2")]
    pub email: String,
    #[prost(int32, tag = "3")]
    pub role: i32,
    #[prost(int64, tag = "4")]
    pub created: i64,
    #[prost(int64, optional, tag = "5")]
    pub last_seen: Option<i64>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Image {
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(uint32, tag = "2")]
    pub width: u32,
    #[prost(uint32, tag = "3")]
    pub height: u32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Post {
    #[prost(bytes = "vec", tag = "1")]
    pub id: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub author: Vec<u8>,
    #[prost(string, tag = "3")]
    pub slug: String,
    #[prost(string, tag = "4")]
    pub title: String,
    #[prost(int32, tag = "5")]
    pub state: i32,
    #[prost(int64, tag = "6")]
    pub created: i64,
    #[prost(int64, tag = "7")]
    pub changed: i64,
    #[prost(message, optional, tag = "8")]
    pub cover: Option<Image>,
    #[prost(string, repeated, tag = "9")]
    pub tags: Vec<String>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Etag {
    #[prost(string, tag = "1")]
    pub value: String,
    #[prost(int64, tag = "2")]
    pub seen_at: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Document {
    #[prost(bytes = "vec", tag = "1")]
    pub id: Vec<u8>,
    #[prost(int32, tag = "2")]
    pub source: i32,
    #[prost(int32, tag = "3")]
    pub kind: i32,
    #[prost(string, tag = "4")]
    pub key: String,
    #[prost(bytes = "vec", optional, tag = "5")]
    pub parent: Option<Vec<u8>>,
    #[prost(uint64, optional, tag = "6")]
    pub seq: Option<u64>,
    #[prost(int32, tag = "7")]
    pub state: i32,
    #[prost(int64, optional, tag = "8")]
    pub fetched_at: Option<i64>,
    #[prost(int64, optional, tag = "9")]
    pub changed_at: Option<i64>,
    #[prost(message, repeated, tag = "10")]
    pub etags: Vec<Etag>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Posts {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<Post>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct ImageV2 {
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(uint32, tag = "2")]
    pub width: u32,
    #[prost(uint32, tag = "3")]
    pub height: u32,
    #[prost(string, optional, tag = "4")]
    pub alt: Option<String>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct PostV2 {
    #[prost(bytes = "vec", tag = "1")]
    pub id: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub author: Vec<u8>,
    #[prost(string, tag = "3")]
    pub slug: String,
    #[prost(string, tag = "4")]
    pub title: String,
    #[prost(int32, tag = "5")]
    pub state: i32,
    #[prost(int64, tag = "6")]
    pub created: i64,
    #[prost(int64, tag = "7")]
    pub changed: i64,
    #[prost(message, optional, tag = "8")]
    pub cover: Option<ImageV2>,
    #[prost(string, repeated, tag = "9")]
    pub tags: Vec<String>,
    #[prost(string, optional, tag = "10")]
    pub summary: Option<String>,
}

fn uuid(bytes: &[u8]) -> Uuid {
    Uuid::from_slice(bytes).expect("16-byte uuid")
}

fn index<T: Copy>(all: &[T], i: i32) -> T {
    all[i as usize]
}

pub trait Proto: Sized {
    type Message: prost::Message + Default + PartialEq;
    fn to_proto(&self) -> Self::Message;
    fn from_proto(m: Self::Message) -> Self;
}

impl Proto for model::User {
    type Message = User;

    fn to_proto(&self) -> User {
        User {
            id: self.id.as_bytes().to_vec(),
            email: self.email.clone(),
            role: self.role as i32,
            created: to_millis(self.created),
            last_seen: self.last_seen.map(to_millis),
        }
    }

    fn from_proto(m: User) -> Self {
        Self {
            id: uuid(&m.id),
            email: m.email,
            role: index(model::Role::ALL, m.role),
            created: from_millis(m.created),
            last_seen: m.last_seen.map(from_millis),
        }
    }
}

fn image_to_proto(i: &model::Image) -> Image {
    Image { key: i.key.clone(), width: i.width.into(), height: i.height.into() }
}

impl Proto for model::Post {
    type Message = Post;

    fn to_proto(&self) -> Post {
        Post {
            id: self.id.as_bytes().to_vec(),
            author: self.author.as_bytes().to_vec(),
            slug: self.slug.clone(),
            title: self.title.clone(),
            state: self.state as i32,
            created: to_millis(self.created),
            changed: to_millis(self.changed),
            cover: self.cover.as_ref().map(image_to_proto),
            tags: self.tags.clone(),
        }
    }

    fn from_proto(m: Post) -> Self {
        Self {
            id: uuid(&m.id),
            author: uuid(&m.author),
            slug: m.slug,
            title: m.title,
            state: index(model::PostState::ALL, m.state),
            created: from_millis(m.created),
            changed: from_millis(m.changed),
            cover: m.cover.map(|i| model::Image { key: i.key, width: i.width as u16, height: i.height as u16 }),
            tags: m.tags,
        }
    }
}

impl Proto for model::Document {
    type Message = Document;

    fn to_proto(&self) -> Document {
        Document {
            id: self.id.as_bytes().to_vec(),
            source: self.source as i32,
            kind: self.kind as i32,
            key: self.key.clone(),
            parent: self.parent.map(|p| p.as_bytes().to_vec()),
            seq: self.seq,
            state: self.state as i32,
            fetched_at: self.fetched_at.map(to_millis),
            changed_at: self.changed_at.map(to_millis),
            etags: self.etags.iter().map(|e| Etag { value: e.value.clone(), seen_at: to_millis(e.seen_at) }).collect(),
        }
    }

    fn from_proto(m: Document) -> Self {
        Self {
            id: uuid(&m.id),
            source: index(model::Source::ALL, m.source),
            kind: index(model::DocumentKind::ALL, m.kind),
            key: m.key,
            parent: m.parent.as_deref().map(uuid),
            seq: m.seq,
            state: index(model::DocumentState::ALL, m.state),
            fetched_at: m.fetched_at.map(from_millis),
            changed_at: m.changed_at.map(from_millis),
            etags: m.etags.into_iter().map(|e| model::Etag { value: e.value, seen_at: from_millis(e.seen_at) }).collect(),
        }
    }
}

impl Proto for Vec<model::Post> {
    type Message = Posts;

    fn to_proto(&self) -> Posts {
        Posts { items: self.iter().map(Proto::to_proto).collect() }
    }

    fn from_proto(m: Posts) -> Self {
        m.items.into_iter().map(model::Post::from_proto).collect()
    }
}

impl PostV2 {
    pub fn from_model(v: &model::PostV2) -> Self {
        Self {
            id: v.id.as_bytes().to_vec(),
            author: v.author.as_bytes().to_vec(),
            slug: v.slug.clone(),
            title: v.title.clone(),
            state: v.state as i32,
            created: to_millis(v.created),
            changed: to_millis(v.changed),
            cover: v.cover.as_ref().map(|i| ImageV2 {
                key: i.key.clone(),
                width: i.width.into(),
                height: i.height.into(),
                alt: i.alt.clone(),
            }),
            tags: v.tags.clone(),
            summary: v.summary.clone(),
        }
    }
}
