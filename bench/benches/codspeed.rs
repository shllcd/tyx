use divan::{Bencher, black_box};
use tyx_bench::data::{self, Lcg};
use tyx_bench::formats::Dataset;
use tyx_bench::model::{Document, Post, User};

fn main() {
    divan::main();
}

fn rng() -> Lcg {
    Lcg::new(0x7a6e_a5ee_d00d)
}

#[divan::bench(types = [User, Post, Document, Vec<Post>])]
fn encode<T: Dataset + Sample>(bencher: Bencher) {
    let value = T::sample();
    let mut buf = Vec::new();
    bencher.bench_local(|| {
        buf.clear();
        tyx::encode_into(black_box(&value), &mut buf);
    });
}

#[divan::bench(types = [User, Post, Document, Vec<Post>])]
fn decode<T: Dataset + Sample>(bencher: Bencher) {
    let bytes = tyx::to_vec(&T::sample());
    bencher.bench_local(|| tyx::from_slice::<T>(black_box(&bytes)).expect("tyx"));
}

#[divan::bench(types = [User, Post, Document, Vec<Post>])]
fn decode_borrowed<T: Dataset + Sample>(bencher: Bencher) {
    let bytes = tyx::to_vec(&T::sample());
    bencher.bench_local(|| drop(black_box(tyx::from_slice::<T::Ref<'_>>(black_box(&bytes)).expect("tyx borrowed"))));
}

trait Sample {
    fn sample() -> Self;
}

impl Sample for User {
    fn sample() -> Self {
        data::user(&mut rng())
    }
}

impl Sample for Post {
    fn sample() -> Self {
        data::post(&mut rng())
    }
}

impl Sample for Document {
    fn sample() -> Self {
        data::document(&mut rng())
    }
}

impl Sample for Vec<Post> {
    fn sample() -> Self {
        data::batch(&mut rng(), 10_000)
    }
}
