//! How much memory one piece of music takes while it plays in a window
//! (docs/phase3-design.md, section 5: "about 0.35 MB", to be measured).
//!
//! This is its own test program, with one test, so that every allocation
//! counted is the music's: the program's allocator counts the bytes in
//! use, and the most there were at once.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

mod common;
use common::TestFolder;

use duckforge::prelude::*;

/// The system's allocator, counting.
struct Counting;

static IN_USE: AtomicUsize = AtomicUsize::new(0);
static MOST: AtomicUsize = AtomicUsize::new(0);

fn grew(by: usize) {
    let now = IN_USE.fetch_add(by, Ordering::Relaxed) + by;
    MOST.fetch_max(now, Ordering::Relaxed);
}

// Safety: every call goes straight to the system's allocator, with the
// same arguments; the counting only adds and takes away numbers.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grew(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            grew(new_size - layout.size());
        } else {
            IN_USE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn one_piece_of_music_takes_under_a_megabyte_while_it_plays() {
    let folder = TestFolder::new("music-memory");
    let path = folder.join("day.ogg");
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/duck_craft/assets/music_day.ogg"
    ))
    .unwrap();
    std::fs::write(&path, &bytes).unwrap();
    drop(bytes);
    let mut block = vec![0.0f32; 2 * 1024];

    // Loading keeps the file's bytes (about 0.3 MB), and checks them.
    let before = IN_USE.load(Ordering::Relaxed);
    let day = Music::load(&path).unwrap();
    let loaded = IN_USE.load(Ordering::Relaxed) - before;

    // Playing: the decoder thread, its ring buffer and lewton's own
    // tables. Three seconds of music taken as a sound thread would.
    let before = IN_USE.load(Ordering::Relaxed);
    MOST.store(before, Ordering::Relaxed);
    let mut read = duckforge::test_hooks::music_stream(&day);
    let mut taken = 0;
    while taken < 3 * 44_100 {
        let got = read(&mut block);
        taken += got;
        if got == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let playing = MOST.load(Ordering::Relaxed) - before;
    drop(read);

    eprintln!(
        "the day theme: {} KB to load (the file), at most {} KB more while it plays",
        loaded / 1000,
        playing / 1000
    );
    assert!(playing < 1_000_000, "{playing} bytes while it plays");
    assert!(
        loaded + playing < 1_000_000,
        "{} bytes in all",
        loaded + playing
    );
    // A Wav of the same music: 1,008,000 frames x 2 x 4 bytes.
    assert!(8_064_000 / (loaded + playing) >= 8, "at least 8 times less");
}
