//! Runs a merge and reports the peak heap it needed, to check it fits in the
//! memory the console can spare.
//!
//!     cargo run --release -p totk-merge --example peak_memory -- <romfs> <work dir> <mod>...

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK.fetch_max(now, Ordering::Relaxed);
        COUNT.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(pointer, layout)
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn main() {
    totk_merge::set_log_sink(|line| {
        println!("  | {} (heap {} MiB, peak {} MiB)", line, CURRENT.load(Ordering::Relaxed) >> 20, PEAK.load(Ordering::Relaxed) >> 20)
    });
    totk_merge::set_verbose(std::env::var("VERBOSE").is_ok());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let romfs = format!("{}/", args[0].trim_end_matches(['/', '\\']));
    let work = PathBuf::from(&args[1]);
    let mut config = totk_merge::config::Config::default();
    config.cache_dir = work.join("cache").to_string_lossy().to_string();
    config.force_merge = true;
    config.use_romfslite = false;
    // "auto" would look for sd:/totk/locale.txt, which a PC does not have.
    config.locales = std::env::var("LOCALES").unwrap_or_else(|_| "all".into());
    let mods = args[2..]
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let path = path.replace('\\', "/");
            let (kind, content) = totk_merge::mods::classify(&path).unwrap();
            totk_merge::mods::ModSpec {
                name: totk_merge::sys::path::file_name(&path).to_string(),
                kind,
                path: content,
                priority: i as i32,
                options: BTreeMap::new(),
                plugins: Vec::new(),
            }
        })
        .collect();
    let plan = totk_merge::mods::Plan {
        mods,
        merged_dir: work.join("merged").to_string_lossy().to_string(),
        profile: "peak".into(),
    };
    let outcome = totk_merge::engine::Engine::new(&config, &romfs).run(&plan);
    println!(
        "served {} files; peak heap {} MiB over {} allocations",
        outcome.redirects.len(),
        PEAK.load(Ordering::Relaxed) >> 20,
        COUNT.load(Ordering::Relaxed)
    );
}
