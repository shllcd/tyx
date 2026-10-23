use rkyv::rancor::Fallible;
use rkyv::with::{ArchiveWith, DeserializeWith, Map, SerializeWith};
use rkyv::{Archived, Place};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

pub fn to_millis(t: OffsetDateTime) -> i64 {
    (t.unix_timestamp_nanos() / 1_000_000) as i64
}

pub fn from_millis(millis: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000).expect("millis in range")
}

pub mod millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::OffsetDateTime;

    pub fn serialize<S: Serializer>(t: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_i64(super::to_millis(*t))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<OffsetDateTime, D::Error> {
        i64::deserialize(d).map(super::from_millis)
    }

    pub mod option {
        use serde::{Deserialize, Deserializer, Serialize, Serializer};
        use time::OffsetDateTime;

        pub fn serialize<S: Serializer>(t: &Option<OffsetDateTime>, s: S) -> Result<S::Ok, S::Error> {
            t.map(super::super::to_millis).serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<OffsetDateTime>, D::Error> {
            Option::<i64>::deserialize(d).map(|m| m.map(super::super::from_millis))
        }
    }
}

pub struct AsMillis;

impl ArchiveWith<OffsetDateTime> for AsMillis {
    type Archived = Archived<i64>;
    type Resolver = ();

    fn resolve_with(field: &OffsetDateTime, (): (), out: Place<Self::Archived>) {
        out.write(Archived::<i64>::from_native(to_millis(*field)));
    }
}

impl<S: Fallible + ?Sized> SerializeWith<OffsetDateTime, S> for AsMillis {
    fn serialize_with(_: &OffsetDateTime, _: &mut S) -> Result<(), S::Error> {
        Ok(())
    }
}

impl<D: Fallible + ?Sized> DeserializeWith<Archived<i64>, OffsetDateTime, D> for AsMillis {
    fn deserialize_with(field: &Archived<i64>, _: &mut D) -> Result<OffsetDateTime, D::Error> {
        Ok(from_millis(field.to_native()))
    }
}

macro_rules! unit_enum {
    ($name:ident { $($variant:ident),+ }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, tyx::Type, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, bitcode::Encode, bitcode::Decode)]
        #[rkyv(derive(Debug, Clone, Copy, PartialEq, Eq))]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &[Self] = &[$(Self::$variant),+];
        }
    };
}

unit_enum!(Role { Reader, Author, Admin });
unit_enum!(PostState { Draft, Published, Hidden });
unit_enum!(Source { Web, Rss, Sitemap, Api });
unit_enum!(DocumentKind { Page, Feed, Image, Pdf });
unit_enum!(DocumentState { Pending, Fetched, Failed, Gone });

#[derive(Debug, Clone, PartialEq, tyx::Type, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub role: Role,
    #[serde(with = "millis")]
    #[rkyv(with = AsMillis)]
    pub created: OffsetDateTime,
    #[serde(with = "millis::option")]
    #[rkyv(with = Map<AsMillis>)]
    pub last_seen: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, tyx::Type, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Image {
    pub key: String,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, tyx::Type, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Post {
    pub id: Uuid,
    pub author: Uuid,
    pub slug: String,
    pub title: String,
    pub state: PostState,
    #[serde(with = "millis")]
    #[rkyv(with = AsMillis)]
    pub created: OffsetDateTime,
    #[serde(with = "millis")]
    #[rkyv(with = AsMillis)]
    pub changed: OffsetDateTime,
    pub cover: Option<Image>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, tyx::Type, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Etag {
    pub value: String,
    #[serde(with = "millis")]
    #[rkyv(with = AsMillis)]
    pub seen_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, tyx::Type, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Document {
    pub id: Uuid,
    pub source: Source,
    pub kind: DocumentKind,
    pub key: String,
    pub parent: Option<Uuid>,
    pub seq: Option<u64>,
    pub state: DocumentState,
    #[serde(with = "millis::option")]
    #[rkyv(with = Map<AsMillis>)]
    pub fetched_at: Option<OffsetDateTime>,
    #[serde(with = "millis::option")]
    #[rkyv(with = Map<AsMillis>)]
    pub changed_at: Option<OffsetDateTime>,
    pub etags: Vec<Etag>,
}

#[derive(Debug, tyx::Type, Serialize, Deserialize)]
pub struct UserRef<'a> {
    pub id: Uuid,
    pub email: &'a str,
    pub role: Role,
    #[serde(with = "millis")]
    pub created: OffsetDateTime,
    #[serde(with = "millis::option")]
    pub last_seen: Option<OffsetDateTime>,
}

#[derive(Debug, tyx::Type, Serialize, Deserialize)]
pub struct ImageRef<'a> {
    pub key: &'a str,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, tyx::Type, Serialize, Deserialize)]
pub struct PostRef<'a> {
    pub id: Uuid,
    pub author: Uuid,
    pub slug: &'a str,
    pub title: &'a str,
    pub state: PostState,
    #[serde(with = "millis")]
    pub created: OffsetDateTime,
    #[serde(with = "millis")]
    pub changed: OffsetDateTime,
    #[serde(borrow)]
    pub cover: Option<ImageRef<'a>>,
    #[serde(borrow)]
    pub tags: Vec<&'a str>,
}

#[derive(Debug, tyx::Type, Serialize, Deserialize)]
pub struct EtagRef<'a> {
    pub value: &'a str,
    #[serde(with = "millis")]
    pub seen_at: OffsetDateTime,
}

#[derive(Debug, tyx::Type, Serialize, Deserialize)]
pub struct DocumentRef<'a> {
    pub id: Uuid,
    pub source: Source,
    pub kind: DocumentKind,
    pub key: &'a str,
    pub parent: Option<Uuid>,
    pub seq: Option<u64>,
    pub state: DocumentState,
    #[serde(with = "millis::option")]
    pub fetched_at: Option<OffsetDateTime>,
    #[serde(with = "millis::option")]
    pub changed_at: Option<OffsetDateTime>,
    #[serde(borrow)]
    pub etags: Vec<EtagRef<'a>>,
}

#[derive(Debug, tyx::Type, Serialize, Deserialize)]
pub struct ImageV2 {
    pub key: String,
    pub width: u16,
    pub height: u16,
    pub alt: Option<String>,
}

#[derive(Debug, tyx::Type, Serialize, Deserialize)]
pub struct PostV2 {
    pub id: Uuid,
    pub author: Uuid,
    pub slug: String,
    pub title: String,
    pub state: PostState,
    #[serde(with = "millis")]
    pub created: OffsetDateTime,
    #[serde(with = "millis")]
    pub changed: OffsetDateTime,
    pub cover: Option<ImageV2>,
    pub tags: Vec<String>,
    pub summary: Option<String>,
}

impl PostV2 {
    pub fn from_v1(post: &Post) -> Self {
        Self {
            id: post.id,
            author: post.author,
            slug: post.slug.clone(),
            title: post.title.clone(),
            state: post.state,
            created: post.created,
            changed: post.changed,
            cover: post.cover.as_ref().map(|c| ImageV2 { key: c.key.clone(), width: c.width, height: c.height, alt: Some("alt text".into()) }),
            tags: post.tags.clone(),
            summary: Some("a summary appended by a newer schema".into()),
        }
    }
}

#[derive(Debug, tyx::Type)]
pub struct Blob {
    pub s: String,
}
