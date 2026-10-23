use crate::model::{self, from_millis, to_millis};
use crate::rows_capnp::{document, image, post, posts, user};
use capnp::message::{self, ReaderOptions, ReaderSegments};
use uuid::Uuid;

pub const OPTIONS: ReaderOptions = ReaderOptions { traversal_limit_in_words: Some(1 << 30), nesting_limit: 64 };

fn split(id: Uuid) -> (u64, u64) {
    id.as_u64_pair()
}

fn join(hi: u64, lo: u64) -> Uuid {
    Uuid::from_u64_pair(hi, lo)
}

pub trait Capn: Sized {
    fn capnp_build<A: message::Allocator>(&self, msg: &mut message::Builder<A>);
    fn capnp_read<S: ReaderSegments>(msg: &message::Reader<S>) -> capnp::Result<Self>;
}

impl Capn for model::User {
    fn capnp_build<A: message::Allocator>(&self, msg: &mut message::Builder<A>) {
        let mut u = msg.init_root::<user::Builder>();
        let (hi, lo) = split(self.id);
        u.set_id_hi(hi);
        u.set_id_lo(lo);
        u.set_email(&self.email[..]);
        u.set_role(crate::rows_capnp::Role::try_from(self.role as u16).expect("role"));
        u.set_created(to_millis(self.created));
        match self.last_seen {
            Some(t) => u.get_last_seen().set_at(to_millis(t)),
            None => u.get_last_seen().set_unset(()),
        }
    }

    fn capnp_read<S: ReaderSegments>(msg: &message::Reader<S>) -> capnp::Result<Self> {
        let u = msg.get_root::<user::Reader>()?;
        Ok(Self {
            id: join(u.get_id_hi(), u.get_id_lo()),
            email: u.get_email()?.to_string()?,
            role: model::Role::ALL[u.get_role()? as usize],
            created: from_millis(u.get_created()),
            last_seen: match u.get_last_seen().which()? {
                user::last_seen::Unset(()) => None,
                user::last_seen::At(t) => Some(from_millis(t)),
            },
        })
    }
}

fn build_post(p: &model::Post, mut b: post::Builder) {
    let (hi, lo) = split(p.id);
    b.set_id_hi(hi);
    b.set_id_lo(lo);
    let (hi, lo) = split(p.author);
    b.set_author_hi(hi);
    b.set_author_lo(lo);
    b.set_slug(&p.slug[..]);
    b.set_title(&p.title[..]);
    b.set_state(crate::rows_capnp::PostState::try_from(p.state as u16).expect("state"));
    b.set_created(to_millis(p.created));
    b.set_changed(to_millis(p.changed));
    if let Some(c) = &p.cover {
        let mut i: image::Builder = b.reborrow().init_cover();
        i.set_key(&c.key[..]);
        i.set_width(c.width);
        i.set_height(c.height);
    }
    let mut tags = b.init_tags(p.tags.len() as u32);
    for (i, t) in p.tags.iter().enumerate() {
        tags.set(i as u32, &t[..]);
    }
}

pub fn read_post(p: post::Reader) -> capnp::Result<model::Post> {
    Ok(model::Post {
        id: join(p.get_id_hi(), p.get_id_lo()),
        author: join(p.get_author_hi(), p.get_author_lo()),
        slug: p.get_slug()?.to_string()?,
        title: p.get_title()?.to_string()?,
        state: model::PostState::ALL[p.get_state()? as usize],
        created: from_millis(p.get_created()),
        changed: from_millis(p.get_changed()),
        cover: if p.has_cover() {
            let c = p.get_cover()?;
            Some(model::Image { key: c.get_key()?.to_string()?, width: c.get_width(), height: c.get_height() })
        } else {
            None
        },
        tags: p.get_tags()?.iter().map(|t| t?.to_string().map_err(Into::into)).collect::<capnp::Result<_>>()?,
    })
}

impl Capn for model::Post {
    fn capnp_build<A: message::Allocator>(&self, msg: &mut message::Builder<A>) {
        build_post(self, msg.init_root());
    }

    fn capnp_read<S: ReaderSegments>(msg: &message::Reader<S>) -> capnp::Result<Self> {
        read_post(msg.get_root()?)
    }
}

impl Capn for model::Document {
    fn capnp_build<A: message::Allocator>(&self, msg: &mut message::Builder<A>) {
        let mut d = msg.init_root::<document::Builder>();
        let (hi, lo) = split(self.id);
        d.set_id_hi(hi);
        d.set_id_lo(lo);
        d.set_source(crate::rows_capnp::Source::try_from(self.source as u16).expect("source"));
        d.set_kind(crate::rows_capnp::DocumentKind::try_from(self.kind as u16).expect("kind"));
        d.set_key(&self.key[..]);
        match self.parent {
            Some(p) => {
                let (hi, lo) = split(p);
                let mut g = d.reborrow().get_parent().init_uuid();
                g.set_hi(hi);
                g.set_lo(lo);
            }
            None => d.reborrow().get_parent().set_unset(()),
        }
        match self.seq {
            Some(s) => d.reborrow().get_seq().set_value(s),
            None => d.reborrow().get_seq().set_unset(()),
        }
        d.set_state(crate::rows_capnp::DocumentState::try_from(self.state as u16).expect("state"));
        match self.fetched_at {
            Some(t) => d.reborrow().get_fetched_at().set_at(to_millis(t)),
            None => d.reborrow().get_fetched_at().set_unset(()),
        }
        match self.changed_at {
            Some(t) => d.reborrow().get_changed_at().set_at(to_millis(t)),
            None => d.reborrow().get_changed_at().set_unset(()),
        }
        let mut etags = d.init_etags(self.etags.len() as u32);
        for (i, e) in self.etags.iter().enumerate() {
            let mut b = etags.reborrow().get(i as u32);
            b.set_value(&e.value[..]);
            b.set_seen_at(to_millis(e.seen_at));
        }
    }

    fn capnp_read<S: ReaderSegments>(msg: &message::Reader<S>) -> capnp::Result<Self> {
        let d = msg.get_root::<document::Reader>()?;
        Ok(Self {
            id: join(d.get_id_hi(), d.get_id_lo()),
            source: model::Source::ALL[d.get_source()? as usize],
            kind: model::DocumentKind::ALL[d.get_kind()? as usize],
            key: d.get_key()?.to_string()?,
            parent: match d.get_parent().which()? {
                document::parent::Unset(()) => None,
                document::parent::Uuid(g) => Some(join(g.get_hi(), g.get_lo())),
            },
            seq: match d.get_seq().which()? {
                document::seq::Unset(()) => None,
                document::seq::Value(s) => Some(s),
            },
            state: model::DocumentState::ALL[d.get_state()? as usize],
            fetched_at: match d.get_fetched_at().which()? {
                document::fetched_at::Unset(()) => None,
                document::fetched_at::At(t) => Some(from_millis(t)),
            },
            changed_at: match d.get_changed_at().which()? {
                document::changed_at::Unset(()) => None,
                document::changed_at::At(t) => Some(from_millis(t)),
            },
            etags: d
                .get_etags()?
                .iter()
                .map(|e| Ok(model::Etag { value: e.get_value()?.to_string()?, seen_at: from_millis(e.get_seen_at()) }))
                .collect::<capnp::Result<_>>()?,
        })
    }
}

impl Capn for Vec<model::Post> {
    fn capnp_build<A: message::Allocator>(&self, msg: &mut message::Builder<A>) {
        let root = msg.init_root::<posts::Builder>();
        let mut items = root.init_items(self.len() as u32);
        for (i, p) in self.iter().enumerate() {
            build_post(p, items.reborrow().get(i as u32));
        }
    }

    fn capnp_read<S: ReaderSegments>(msg: &message::Reader<S>) -> capnp::Result<Self> {
        msg.get_root::<posts::Reader>()?.get_items()?.iter().map(read_post).collect()
    }
}

pub fn touch(p: post::Reader) -> capnp::Result<u64> {
    let cover = if p.has_cover() {
        let c = p.get_cover()?;
        c.get_key()?.to_str()?.len() as u64 + u64::from(c.get_width()) + u64::from(c.get_height())
    } else {
        0
    };
    let mut tags = 0;
    for t in p.get_tags()? {
        tags += t?.to_str()?.len() as u64;
    }
    Ok(p.get_id_hi()
        + p.get_author_hi()
        + p.get_slug()?.to_str()?.len() as u64
        + p.get_title()?.to_str()?.len() as u64
        + p.get_state()? as u64
        + p.get_created() as u64
        + p.get_changed() as u64
        + cover
        + tags)
}
