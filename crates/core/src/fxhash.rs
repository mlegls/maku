//! In-repo Fx hash: the rustc-hash multiply-rotate fold. Not DoS-resistant;
//! for interpreter-internal maps keyed on small Copy types and interned
//! strings, where SipHash startup cost dominates lookups.

use std::hash::{BuildHasherDefault, Hasher};

pub type FxBuildHasher = BuildHasherDefault<FxHasher>;
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuildHasher>;
pub type FxHashSet<T> = std::collections::HashSet<T, FxBuildHasher>;

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Clone, Copy, Default)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add_to_hash(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(5) ^ i).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add_to_hash(u64::from_ne_bytes(chunk.try_into().unwrap()));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            self.add_to_hash(buf.len() as u64 ^ u64::from_ne_bytes(buf));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add_to_hash(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{BuildHasher, Hash};

    #[test]
    fn deterministic_and_map_roundtrips() {
        let build = FxBuildHasher::default();
        let mut first = build.build_hasher();
        let mut second = build.build_hasher();
        42_u64.hash(&mut first);
        42_u64.hash(&mut second);
        assert_eq!(first.finish(), second.finish());

        let mut map: FxHashMap<u64, u32> = FxHashMap::default();
        for (key, value) in [(1, 10), (2, 20), (5, 50), (8, 80)] {
            map.insert(key, value);
        }
        for (key, value) in [(1, 10), (2, 20), (5, 50), (8, 80)] {
            assert_eq!(map.get(&key), Some(&value));
        }
    }
}
