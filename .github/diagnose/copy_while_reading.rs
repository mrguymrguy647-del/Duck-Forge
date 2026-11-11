//! Windows diagnosis (temporary): does `fs::copy` of a file make other
//! threads' reads of that same file fail? Usage: copy_while_reading FILE SECONDS
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let source = Arc::new(args[1].clone());
    let seconds: u64 = args[2].parse().unwrap();
    let mode = args.get(3).cloned().unwrap_or_else(|| "copy".into());
    let stop = Arc::new(AtomicBool::new(false));
    let errors: Arc<Mutex<BTreeMap<String, u64>>> = Arc::default();
    let reads = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let dir = std::env::temp_dir().join(format!("copy-diag-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let mut handles = Vec::new();
    // The "copier": what a_picture_of_the_wrong_size_is_a_clear_error does.
    {
        let (source, stop, errors, dir, mode) =
            (source.clone(), stop.clone(), errors.clone(), dir.clone(), mode.clone());
        handles.push(std::thread::spawn(move || {
            let mut n = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let to = dir.join(format!("copy-{}.json", n % 4));
                let result = if mode == "copy" {
                    std::fs::copy(&*source, &to).map(|_| ())
                } else {
                    std::fs::read(&*source).and_then(|bytes| std::fs::write(&to, bytes))
                };
                if let Err(e) = result {
                    *errors.lock().unwrap().entry(format!("{mode}: {e}")).or_default() += 1;
                }
                n += 1;
            }
            println!("{mode} ran {n} times");
        }));
    }
    // The readers: what the other spritesheet and tileset tests do.
    for _ in 0..6 {
        let (source, stop, errors, reads) =
            (source.clone(), stop.clone(), errors.clone(), reads.clone());
        handles.push(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match std::fs::read_to_string(&*source) {
                    Ok(text) if text.is_empty() => {
                        *errors.lock().unwrap().entry("read: empty".into()).or_default() += 1
                    }
                    Ok(_) => {}
                    Err(e) => {
                        *errors.lock().unwrap().entry(format!("read: {e}")).or_default() += 1
                    }
                }
                reads.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(seconds) {
        std::thread::sleep(Duration::from_millis(100));
    }
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        h.join().unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
    println!("{} reads", reads.load(Ordering::Relaxed));
    let errors = errors.lock().unwrap();
    if errors.is_empty() {
        println!("no errors");
    }
    for (what, count) in errors.iter() {
        println!("{count:>8} x {what}");
    }
}
