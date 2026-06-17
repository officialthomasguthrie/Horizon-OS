// Arithmetic in GF(2^8) with the AES reduction polynomial (0x11b), the field
// Shamir's scheme runs over here. Addition is xor; multiplication goes through
// log/exp tables built from the generator 3, which is primitive for this poly
// (2 is not: its order is only 51, so it would not generate the whole field).

const POLY: u16 = 0x11b;

pub struct Gf {
    exp: [u8; 512],
    log: [u8; 256],
}

impl Gf {
    pub fn new() -> Gf {
        let mut exp = [0u8; 512];
        let mut log = [0u8; 256];
        let mut x: u16 = 1;
        for (i, slot) in exp.iter_mut().enumerate().take(255) {
            *slot = x as u8;
            log[x as usize] = i as u8;
            // Multiply by the generator 3 = (x * 2) xor x, then reduce.
            x ^= x << 1;
            if x & 0x100 != 0 {
                x ^= POLY;
            }
        }
        // Double the exp table so a + b (both < 255) never needs a modulo.
        for i in 255..512 {
            exp[i] = exp[i - 255];
        }
        Gf { exp, log }
    }

    pub fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            return 0;
        }
        self.exp[self.log[a as usize] as usize + self.log[b as usize] as usize]
    }

    pub fn div(&self, a: u8, b: u8) -> u8 {
        // Caller guarantees b != 0 (denominators are differences of distinct xs).
        if a == 0 {
            return 0;
        }
        self.exp[self.log[a as usize] as usize + 255 - self.log[b as usize] as usize]
    }

    // Evaluate a polynomial (coeffs low-order first) at x by Horner's method.
    pub fn eval(&self, coeffs: &[u8], x: u8) -> u8 {
        let mut acc = 0u8;
        for &c in coeffs.iter().rev() {
            acc = self.mul(acc, x) ^ c;
        }
        acc
    }
}
