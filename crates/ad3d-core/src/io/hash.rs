//! FNV-1a 64-bit: a tiny, dependency-free content hash for golden-file tests
//! and for checking that a project's embedded image is the one it recorded.
//! Not cryptographic; it guards against accidents, not attackers.

use std::io::Write;

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// Streaming FNV-1a 64. Implements [`Write`] so writers can hash what they emit.
#[derive(Debug, Clone, Copy)]
pub struct Fnv64(u64);

impl Default for Fnv64 {
    fn default() -> Self {
        Fnv64(OFFSET)
    }
}

impl Fnv64 {
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub fn update(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(PRIME);
        }
        self.0 = h;
    }
    pub fn finish(&self) -> u64 {
        self.0
    }
    /// 16 lowercase hex digits.
    pub fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}

impl Write for Fnv64 {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// FNV-1a 64 of `bytes` as 16 hex digits.
pub fn fnv64_hex(bytes: &[u8]) -> String {
    let mut h = Fnv64::new();
    h.update(bytes);
    h.hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(fnv64_hex(b""), "cbf29ce484222325");
        assert_eq!(fnv64_hex(b"a"), "af63dc4c8601ec8c");
        assert_eq!(fnv64_hex(b"foobar"), "85944171f73967e8");
    }
}
