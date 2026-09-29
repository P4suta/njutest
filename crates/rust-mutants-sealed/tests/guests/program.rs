// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A guest program for the sealed host, compiled for `wasm32-wasip1` by the test that runs it: its first argument names what it does.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::time::{Duration, Instant, SystemTime};

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let mode = arguments.get(1).map_or("", String::as_str);
    let rest = arguments.get(2..).unwrap_or_default();
    match mode {
        "echo" => echo(rest),
        "exit" => exit(rest),
        "panic" => panic!("the guest panicked on purpose"),
        "spin-forever" => spin_forever(),
        "allocate" => allocate(rest),
        "recurse" => recurse(),
        "sleep" => sleep(rest),
        "spin" => spin(rest),
        "random" => random(),
        "read" => read(rest),
        "read-joined" => read_joined(rest),
        "relay" => relay(rest),
        "populate" => populate(rest),
        "write" => write(rest),
        "stat" => stat(rest),
        "rearrange" => rearrange(rest),
        "temp-dir" => println!("{}", std::env::temp_dir().display()),
        "scratch" => scratch(rest),
        unknown => fail(&format!("no mode {unknown:?}")),
    }
}

fn fail(said: &str) -> ! {
    eprintln!("{said}");
    std::process::exit(2)
}

fn number(rest: &[String], at: usize) -> u64 {
    match rest.get(at).map(|text| text.parse::<u64>()) {
        Some(Ok(number)) => number,
        Some(Err(error)) => fail(&format!("argument {at} is not a number: {error}")),
        None => fail(&format!("argument {at} is missing")),
    }
}

fn text(rest: &[String], at: usize) -> &str {
    match rest.get(at) {
        Some(text) => text,
        None => fail(&format!("argument {at} is missing")),
    }
}

fn exit(rest: &[String]) -> ! {
    match i32::try_from(number(rest, 0)) {
        Ok(code) => std::process::exit(code),
        Err(error) => fail(&format!("{error}")),
    }
}

fn echo(rest: &[String]) {
    for argument in rest {
        println!("argument {argument}");
    }
    for (name, value) in std::env::vars() {
        println!("variable {name}={value}");
    }
}

fn spin_forever() {
    loop {
        std::hint::black_box(());
    }
}

fn allocate(rest: &[String]) {
    let bytes = match usize::try_from(number(rest, 0)) {
        Ok(bytes) => bytes,
        Err(error) => fail(&format!("{error}")),
    };
    let held = vec![7_u8; bytes];
    println!("held {}", held.iter().map(|byte| u64::from(*byte)).sum::<u64>());
}

fn recurse() {
    println!("{}", descend(0));
}

fn descend(depth: u32) -> u32 {
    if depth == u32::MAX {
        return 0;
    }
    descend(depth + 1).rotate_left(1) ^ depth
}

fn nanos(duration: Duration) -> u128 {
    duration.as_nanos()
}

fn since_epoch(time: SystemTime) -> u128 {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => nanos(since),
        Err(error) => fail(&format!("{error}")),
    }
}

fn sleep(rest: &[String]) {
    let asked = Duration::from_millis(number(rest, 0));
    let wall_before = SystemTime::now();
    let before = Instant::now();
    std::thread::sleep(asked);
    let after = Instant::now();
    let wall_after = SystemTime::now();
    println!("monotonic {}", nanos(after.duration_since(before)));
    println!("realtime {}", since_epoch(wall_after) - since_epoch(wall_before));
    println!("started {}", since_epoch(wall_before));
}

fn spin(rest: &[String]) {
    let deadline = Instant::now() + Duration::from_millis(number(rest, 0));
    let mut turns = 0_u64;
    while Instant::now() < deadline {
        turns += 1;
    }
    println!("spun {}", turns > 0);
}

fn random() {
    let first = RandomState::new().hash_one(0_u64);
    let second = RandomState::new().hash_one(0_u64);
    println!("random {first} {second}");
}

fn read(rest: &[String]) {
    match std::fs::read_to_string(text(rest, 0)) {
        Ok(contents) => print!("{contents}"),
        Err(error) => fail(&format!("{error}")),
    }
}

fn read_joined(rest: &[String]) {
    let joined = std::path::Path::new(text(rest, 0)).join(text(rest, 1));
    println!("{}", joined.display());
    match std::fs::read_to_string(&joined) {
        Ok(contents) => print!("{contents}"),
        Err(error) => fail(&format!("{error}")),
    }
}

fn relay(rest: &[String]) {
    if let Err(error) = std::fs::write(text(rest, 0), text(rest, 2)) {
        fail(&format!("{error}"));
    }
    read(rest.get(1..2).unwrap_or_default());
}

fn populate(rest: &[String]) {
    let directory = text(rest, 0);
    for name in rest.get(1..).unwrap_or_default() {
        if let Err(error) = std::fs::write(format!("{directory}/{name}"), name) {
            fail(&format!("{error}"));
        }
    }
    let listing = match std::fs::read_dir(directory) {
        Ok(listing) => listing,
        Err(error) => fail(&format!("{error}")),
    };
    for entry in listing {
        match entry {
            Ok(entry) => println!("{}", entry.file_name().display()),
            Err(error) => fail(&format!("{error}")),
        }
    }
}

fn write(rest: &[String]) {
    let path = text(rest, 0);
    match std::fs::metadata(path) {
        Ok(_metadata) => println!("present"),
        Err(_absent) => println!("absent"),
    }
    if let Err(error) = std::fs::write(path, text(rest, 1)) {
        fail(&format!("{error}"));
    }
    read(rest.get(..1).unwrap_or_default());
}

fn stat(rest: &[String]) {
    let metadata = match std::fs::metadata(text(rest, 0)) {
        Ok(metadata) => metadata,
        Err(error) => fail(&format!("{error}")),
    };
    let modified = match metadata.modified() {
        Ok(modified) => since_epoch(modified),
        Err(error) => fail(&format!("{error}")),
    };
    println!("len {} directory {} modified {modified}", metadata.len(), metadata.is_dir());
}

fn step(said: &str, done: std::io::Result<()>) {
    if let Err(error) = done {
        fail(&format!("{said}: {error}"));
    }
}

fn rearrange(rest: &[String]) {
    let base = text(rest, 0);
    let at = |relative: &str| format!("{base}/{relative}");
    step("mkdir out", std::fs::create_dir(at("out")));
    step("write a", std::fs::write(at("out/a.txt"), "moved"));
    step("rename a", std::fs::rename(at("out/a.txt"), at("out/b.txt")));
    step("write c", std::fs::write(at("out/c.txt"), "gone"));
    step("remove c", std::fs::remove_file(at("out/c.txt")));
    step("mkdir empty", std::fs::create_dir(at("out/empty")));
    step("rmdir empty", std::fs::remove_dir(at("out/empty")));
    step(
        "truncate hello",
        std::fs::OpenOptions::new()
            .write(true)
            .open(at("data/hello.txt"))
            .and_then(|file| file.set_len(5)),
    );
    step("remove bye", std::fs::remove_file(at("data/remove-me.txt")));
    step(
        "touch b",
        std::fs::OpenOptions::new()
            .write(true)
            .open(at("out/b.txt"))
            .and_then(|file| file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1000))),
    );
    for relative in ["out/b.txt", "data/hello.txt", "data/remove-me.txt", "out/c.txt"] {
        match std::fs::read_to_string(at(relative)) {
            Ok(contents) => println!("{relative} holds {contents:?}"),
            Err(error) => println!("{relative} is {:?}", error.kind()),
        }
    }
}

fn scratch(rest: &[String]) {
    let Some(directory) = std::env::var_os(text(rest, 0)) else {
        fail("the variable naming the directory is not set")
    };
    let path = std::path::Path::new(&directory).join("scratch.txt");
    step("write", std::fs::write(&path, "kept for a moment"));
    match std::fs::read_to_string(&path) {
        Ok(contents) => println!("read {contents:?}"),
        Err(error) => fail(&format!("read: {error}")),
    }
    step("remove", std::fs::remove_file(&path));
    println!("present afterwards {}", path.exists());
}
