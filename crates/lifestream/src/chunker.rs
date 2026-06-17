use std::ops::Range;

// Content defined chunking, FastCDC style. Cut points depend on the bytes, so
// editing part of a file only rewrites the chunks that actually changed.

const MIN: usize = 2 * 1024;
const NORMAL: usize = 8 * 1024;
const MAX: usize = 64 * 1024;

// Masks tuned for an 8 KiB average chunk (values from the FastCDC paper). The
// stricter mask runs before NORMAL and the looser one after, which pulls chunk
// sizes toward the average.
const MASK_S: u64 = 0x0003_5907_0353_0000;
const MASK_L: u64 = 0x0000_d903_0353_0000;

pub struct Chunker {
    gear: [u64; 256],
}

impl Chunker {
    pub fn new() -> Chunker {
        Chunker { gear: gear_table() }
    }

    pub fn split(&self, data: &[u8]) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut pos = 0;
        while pos < data.len() {
            let len = self.cut(&data[pos..]);
            out.push(pos..pos + len);
            pos += len;
        }
        out
    }

    fn cut(&self, src: &[u8]) -> usize {
        let mut n = src.len();
        if n <= MIN {
            return n;
        }
        if n > MAX {
            n = MAX;
        }
        let center = if n < NORMAL { n } else { NORMAL };
        let mut hash = 0u64;
        let mut i = MIN;
        while i < center {
            hash = (hash << 1).wrapping_add(self.gear[src[i] as usize]);
            if hash & MASK_S == 0 {
                return i;
            }
            i += 1;
        }
        while i < n {
            hash = (hash << 1).wrapping_add(self.gear[src[i] as usize]);
            if hash & MASK_L == 0 {
                return i;
            }
            i += 1;
        }
        n
    }
}

impl Default for Chunker {
    fn default() -> Chunker {
        Chunker::new()
    }
}

// Fixed gear table (splitmix64) so boundaries are identical on every machine.
fn gear_table() -> [u64; 256] {
    let mut t = [0u64; 256];
    let mut x = 0x2545_f491_4f6c_dd1du64;
    for e in t.iter_mut() {
        x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        *e = z ^ (z >> 31);
    }
    t
}
