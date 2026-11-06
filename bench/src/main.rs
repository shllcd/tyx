use tyx_bench::{data, meter, model, report};

#[global_allocator]
static ALLOC: meter::Counting = meter::Counting;

fn main() {
    let started = std::time::Instant::now();
    let mut rng = data::Lcg::new(data::SEED);
    let user = data::user(&mut rng);
    let user_none = model::User { last_seen: None, ..user.clone() };
    let post = data::post(&mut rng);
    let post_none = model::Post { cover: None, ..post.clone() };
    let document = data::document(&mut rng);
    let document_none = model::Document { parent: None, seq: None, fetched_at: None, changed_at: None, ..document.clone() };
    let batch = data::batch(&mut rng, data::BATCH_LEN);

    println!("# Serialization benchmark: tyx vs. other formats\n");
    report::rows("User", &user, &user_none, "last_seen = None");
    report::rows("Post", &post, &post_none, "cover = None");
    report::rows("Document", &document, &document_none, "parent/seq/fetched_at/changed_at = None");
    report::post_access(&post);
    report::batch(&batch);
    report::widening(&post);
    report::evolution(&post);
    println!("\nTotal runtime: {:.1} s", started.elapsed().as_secs_f64());
}
