use std::{io::Write, num::Wrapping, str};

use chrono::Datelike;
use openssl::symm::{decrypt, encrypt, Cipher};

const CRYPTO_KEY: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const PRIME_10K: u32 = 104729;
const PRIME_20K: u32 = 224737;
const PRIME_30K: u32 = 350377;

pub fn simple_decrypt_cipher(cipher: Cipher, data: &[u8], key: &[u8; 16]) -> String {
    let data = get_array(str::from_utf8(data).unwrap());
    let decrypted_data = decrypt(cipher, key, Some(&[0; 16]), &data).unwrap();
    String::from_utf8(decrypted_data).unwrap()
}

pub fn simple_decrypt(data: &[u8], key: &[u8; 16]) -> String {
    let cipher = Cipher::aes_128_ecb();
    simple_decrypt_cipher(cipher, data, key)
}

pub fn simple_encrypt(data: &[u8], key: &[u8; 16]) -> String {
    let cipher = Cipher::aes_128_ecb();
    let encrypted_data = encrypt(cipher, key, None, data).unwrap();
    hex::encode(encrypted_data)
}

pub fn check_challenge_response(response: &str, challenge: &str) -> bool {
    simple_decrypt(response.as_bytes(), &CRYPTO_KEY).as_bytes() == challenge.as_bytes()
}

pub fn make_challenge_response(challenge: &str) -> String {
    simple_encrypt(challenge.as_bytes(), &CRYPTO_KEY)
}

pub fn get_lsx_key(seed: u16) -> [u8; 16] {
    if seed == 0 {
        return CRYPTO_KEY;
    }

    let mut crand = CRandom::default();
    crand.seed(7);
    let seed = (crand.rand() as u32) + (seed as u32);
    crand.seed(seed);

    let mut result: [u8; 16] = [0; 16];
    for i in 0..16 {
        result[i] = crand.rand() as u8;
    }
    result
}

#[derive(Default)]
struct CRandom {
    seed: Wrapping<u32>,
}

impl CRandom {
    fn seed(&mut self, seed: u32) {
        self.seed = Wrapping(seed);
    }

    fn rand(&mut self) -> i32 {
        self.seed = self.seed * Wrapping(214013) + Wrapping(2531011);
        ((self.seed.0 >> 16) & 0xFFFF) as i32
    }
}

fn get_array(s: &str) -> Vec<u8> {
    let mut m = std::io::Cursor::new(Vec::new());
    let mut sb = String::new();
    let filter = "0123456789abcdef";
    let s = s.to_lowercase();
    for c in s.chars() {
        if filter.contains(c) {
            sb.push(c);
        }
    }
    if sb.len() % 2 != 0 {
        return Vec::new();
    }
    let s = sb.as_str();
    for i in 0..(sb.len() / 2) {
        let byte_str = &s[(i * 2)..((i * 2) + 2)];
        let byte = u8::from_str_radix(byte_str, 16).expect("Failed to parse byte from string");
        m.write(&[byte]).expect("Failed to write byte to stream");
    }
    m.into_inner()
}

/// This code is required to launch games and changes daily
pub fn get_rtp_handshake() -> u32 {
    let current_date = chrono::Utc::now();

    let time = (PRIME_10K * current_date.year() as u32)
        ^ (current_date.month() * PRIME_20K)
        ^ (current_date.day() * PRIME_30K);
    time ^ (time << 16) ^ (time >> 16)
}
