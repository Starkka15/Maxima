use crc32fast::Hasher;

use std::fs::File;
use std::io::{self, Read};
use std::num::Wrapping;
use std::path::Path;

pub fn hash_fnv1a(input: &[u8]) -> u64 {
    static OFFSET: u64 = 0xcbf29ce484222325;
    input
        .iter()
        .map(|val| (*val) as u64)
        .fold(OFFSET, |acc, val: u64| {
            (Wrapping(acc ^ val) * Wrapping(0x100000001b3)).0
        })
}

pub fn hash_file_crc32<P: AsRef<Path>>(path: P) -> io::Result<u32> {
    const CHUNK_SIZE: usize = 1_000_000; // 1MB

    let mut hasher = Hasher::new();
    let mut file = File::open(path)?;
    let mut buffer = vec![0; CHUNK_SIZE];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize())
}
