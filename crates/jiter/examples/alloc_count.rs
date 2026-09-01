//! Count the allocator traffic of `JsonValue::parse` over the benchmark documents. Unlike a
//! timing run this is deterministic, so it can be trusted on a busy machine.
#![allow(clippy::print_stdout)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(layout.size(), Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        REALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(new_size, Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn main() {
    let mut names: Vec<String> = std::fs::read_dir("crates/jiter/benches")
        .or_else(|_| std::fs::read_dir("benches"))
        .expect("run from the repository root")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "json").then(|| path.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();

    println!("{:<28} {:>10} {:>10} {:>12}", "document", "allocs", "reallocs", "bytes");
    let (mut total_allocs, mut total_reallocs, mut total_bytes) = (0, 0, 0);
    for name in &names {
        let json_data = std::fs::read(name).unwrap();
        let short = name.rsplit('/').next().unwrap().trim_end_matches(".json").to_string();
        ALLOCS.store(0, Relaxed);
        REALLOCS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        let value = jiter::JsonValue::parse(&json_data, false).unwrap();
        let (allocs, reallocs, bytes) = (ALLOCS.load(Relaxed), REALLOCS.load(Relaxed), BYTES.load(Relaxed));
        drop(value);
        println!("{short:<28} {allocs:>10} {reallocs:>10} {bytes:>12}");
        total_allocs += allocs;
        total_reallocs += reallocs;
        total_bytes += bytes;
    }
    println!(
        "{:<28} {total_allocs:>10} {total_reallocs:>10} {total_bytes:>12}",
        "TOTAL"
    );

    // and the whole json-cases corpus as one number, if it is available
    let Some(root) = std::env::var_os("JSON_CASES") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let index = std::fs::read(root.join("cases.json")).unwrap();
    let cases: serde_json::Value = serde_json::from_slice(&index).unwrap();
    let documents: Vec<Vec<u8>> = cases
        .as_array()
        .unwrap()
        .iter()
        .map(|case| {
            // cases.json holds absolute paths from the checkout that generated it; re-root them
            // like tests/corpus does
            let path = case["path"].as_str().unwrap();
            let rel = match std::path::Path::new(path).strip_prefix(&root) {
                Ok(rel) => rel.to_string_lossy().into_owned(),
                Err(_) => match path.find("/cases/") {
                    Some(index) => path[index + 1..].to_string(),
                    None => path.to_string(),
                },
            };
            std::fs::read(root.join(rel)).unwrap()
        })
        .collect();
    ALLOCS.store(0, Relaxed);
    REALLOCS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    for json_data in &documents {
        drop(jiter::JsonValue::parse(json_data, false));
    }
    println!(
        "{:<28} {:>10} {:>10} {:>12}",
        format!("corpus ({} docs)", documents.len()),
        ALLOCS.load(Relaxed),
        REALLOCS.load(Relaxed),
        BYTES.load(Relaxed)
    );
}
