use crate::bit::Bit;
use crate::capn::{self, Capn};
use crate::flat::Flat;
use crate::model::{Document, DocumentRef, Post, PostRef, User, UserRef};
use crate::proto::Proto;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

pub trait Dataset: Clone + PartialEq + Debug + Serialize + DeserializeOwned + tyx::Encode + for<'de> tyx::Decode<'de> + Proto + Flat + Capn + Bit {
    type Ref<'a>: tyx::Encode + tyx::Decode<'a> + Serialize + Deserialize<'a>;
    fn rkyv_encode(&self, buf: Vec<u8>) -> Vec<u8>;
    fn rkyv_decode(bytes: &[u8]) -> Self;
}

macro_rules! dataset {
    ($ty:ty, $r:ty) => {
        impl Dataset for $ty {
            type Ref<'a> = $r;

            fn rkyv_encode(&self, buf: Vec<u8>) -> Vec<u8> {
                rkyv::api::high::to_bytes_in::<_, rkyv::rancor::Error>(self, buf).expect("rkyv encode")
            }

            fn rkyv_decode(bytes: &[u8]) -> Self {
                rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes).expect("rkyv decode")
            }
        }
    };
}

dataset!(User, UserRef<'a>);
dataset!(Post, PostRef<'a>);
dataset!(Document, DocumentRef<'a>);
dataset!(Vec<Post>, Vec<PostRef<'a>>);

pub trait Visit<T> {
    fn native<M>(
        &mut self,
        name: &'static str,
        to_native: impl Fn(&T) -> M,
        from_native: impl Fn(M) -> T,
        enc: impl FnMut(&M, &mut Vec<u8>),
        dec: impl Fn(&[u8]) -> M,
    );

    fn format(&mut self, name: &'static str, enc: impl FnMut(&T, &mut Vec<u8>), dec: impl Fn(&[u8]) -> T)
    where
        T: Clone,
    {
        self.native(name, T::clone, |x| x, enc, dec);
    }

    fn borrowed(&mut self, dec: impl Fn(&[u8]), reencode: impl Fn(&[u8]) -> Vec<u8>);
}

fn bincode_config() -> bincode::config::Configuration {
    bincode::config::standard()
}

pub fn for_each<T: Dataset>(v: &mut impl Visit<T>) {
    v.format("tyx", |x, buf| tyx::encode_into(x, buf), |b| tyx::from_slice(b).expect("tyx"));
    v.borrowed(
        |b| drop(std::hint::black_box(tyx::from_slice::<T::Ref<'_>>(b).expect("tyx borrowed"))),
        |b| tyx::to_vec(&tyx::from_slice::<T::Ref<'_>>(b).expect("tyx borrowed")),
    );
    v.format(
        "bincode 2 (serde)",
        |x, buf| {
            bincode::serde::encode_into_std_write(x, buf, bincode_config()).expect("bincode");
        },
        |b| bincode::serde::decode_from_slice(b, bincode_config()).expect("bincode").0,
    );
    v.borrowed(
        |b| drop(std::hint::black_box(bincode::serde::borrow_decode_from_slice::<T::Ref<'_>, _>(b, bincode_config()).expect("bincode borrowed"))),
        |b| {
            let r = bincode::serde::borrow_decode_from_slice::<T::Ref<'_>, _>(b, bincode_config()).expect("bincode borrowed").0;
            bincode::serde::encode_to_vec(&r, bincode_config()).expect("bincode")
        },
    );
    v.format(
        "postcard",
        |x, buf| *buf = postcard::to_extend(x, std::mem::take(buf)).expect("postcard"),
        |b| postcard::from_bytes(b).expect("postcard"),
    );
    v.borrowed(
        |b| drop(std::hint::black_box(postcard::from_bytes::<T::Ref<'_>>(b).expect("postcard borrowed"))),
        |b| postcard::to_allocvec(&postcard::from_bytes::<T::Ref<'_>>(b).expect("postcard borrowed")).expect("postcard"),
    );
    let mut bitcode_buf = bitcode::Buffer::new();
    v.native(
        "bitcode (derive)",
        T::to_bit,
        T::from_bit,
        |x, buf| buf.extend_from_slice(bitcode_buf.encode(x)),
        |b| bitcode::decode(b).expect("bitcode"),
    );
    v.format("bitcode (serde)", |x, buf| *buf = bitcode::serialize(x).expect("bitcode"), |b| bitcode::deserialize(b).expect("bitcode"));
    v.format("rkyv", |x, buf| *buf = x.rkyv_encode(std::mem::take(buf)), |b| T::rkyv_decode(b));
    v.native(
        "protobuf (prost)",
        T::to_proto,
        T::from_proto,
        |x, buf| prost::Message::encode(x, buf).expect("prost"),
        |b| prost::Message::decode(b).expect("prost"),
    );
    let mut fbb = flatbuffers::FlatBufferBuilder::with_capacity(1024);
    v.format(
        "flatbuffers",
        |x, buf| {
            fbb.reset();
            x.fb_build(&mut fbb);
            buf.extend_from_slice(fbb.finished_data());
        },
        |b| T::fb_read(b),
    );
    let mut scratch = vec![0u8; 4096];
    let mut scratch_alloc = capnp::message::ScratchSpaceHeapAllocator::new(&mut scratch);
    v.format(
        "capnp",
        |x, buf| {
            let mut msg = capnp::message::Builder::new(&mut scratch_alloc);
            x.capnp_build(&mut msg);
            capnp::serialize::write_message(&mut *buf, &msg).expect("capnp");
        },
        |b| {
            let mut slice = b;
            let msg = capnp::serialize::read_message_from_flat_slice_no_alloc(&mut slice, capn::OPTIONS).expect("capnp");
            T::capnp_read(&msg).expect("capnp")
        },
    );
    let mut packed_alloc = capnp::message::HeapAllocator::new();
    v.format(
        "capnp packed",
        |x, buf| {
            let mut msg = capnp::message::Builder::new(&mut packed_alloc);
            x.capnp_build(&mut msg);
            capnp::serialize_packed::write_message(&mut *buf, &msg).expect("capnp");
        },
        |b| {
            let msg = capnp::serialize_packed::read_message(b, capn::OPTIONS).expect("capnp");
            T::capnp_read(&msg).expect("capnp")
        },
    );
    v.format("pot", |x, buf| pot::to_writer(x, &mut *buf).expect("pot"), |b| pot::from_slice(b).expect("pot"));
    v.format(
        "msgpack (rmp, array)",
        |x, buf| rmp_serde::encode::write(&mut *buf, x).expect("rmp"),
        |b| rmp_serde::from_slice(b).expect("rmp"),
    );
    v.format(
        "msgpack (rmp, named)",
        |x, buf| rmp_serde::encode::write_named(&mut *buf, x).expect("rmp"),
        |b| rmp_serde::from_slice(b).expect("rmp"),
    );
    v.format("cbor (ciborium)", |x, buf| ciborium::into_writer(x, &mut *buf).expect("cbor"), |b| ciborium::from_reader(b).expect("cbor"));
    v.format("json (serde_json)", |x, buf| serde_json::to_writer(&mut *buf, x).expect("json"), |b| serde_json::from_slice(b).expect("json"));
}
