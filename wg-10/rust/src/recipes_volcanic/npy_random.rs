//! NumPy-compatible random stream used by the volcanic recipe.

/// numpy SeedSequence hashmix constants (`bit_generator.pyx`).
const XSHIFT: u32 = 16; // np.uint32 itemsize*8 / 2
const MULT_A: u32 = 0x931e_8875; // hashmix multiplier
const MULT_B: u32 = 0x58f3_8ded; // generate_state multiplier
const MIX_MULT_L: u32 = 0xca01_f9dd;
const MIX_MULT_R: u32 = 0x4973_f715;
const INIT_A: u32 = 0x43b0_d7e5; // hashmix initial hash_const
const INIT_B: u32 = 0x8b51_f9dd; // generate_state initial hash_const

/// numpy SeedSequence: derives a pool from the integer entropy, then streams 32-bit
/// words via `generate_state`. Mirror of numpy `SeedSequence` for the
/// `default_rng(int)` case (entropy = the int split into little-endian u32 words;
/// spawn_key empty; pool_size 4).
pub struct SeedSequence {
    pool: [u32; 4],
}

impl SeedSequence {
    /// `SeedSequence(entropy=<nonneg int>)`. The integer is decomposed into
    /// little-endian 32-bit words (numpy's `_int_to_uint32_array`); a zero integer
    /// becomes a single 0 word.
    pub fn new(entropy: u128) -> Self {
        let assembled = int_to_uint32_array(entropy);
        // numpy mixes assembled_entropy = entropy + spawn_key; spawn_key is empty here.
        let pool = mix_entropy(&assembled);
        SeedSequence { pool }
    }

    /// `generate_state(n_words, np.uint32)` — the hashmix output stream.
    pub fn generate_state(&self, n_words: usize) -> Vec<u32> {
        let mut state = vec![0u32; n_words];
        let mut hash_const: u32 = INIT_B;
        let src_size = self.pool.len();
        for (i_dst, out) in state.iter_mut().enumerate() {
            let mut data_val = self.pool[i_dst % src_size];
            data_val ^= hash_const;
            hash_const = hash_const.wrapping_mul(MULT_B);
            data_val = data_val.wrapping_mul(hash_const);
            data_val ^= data_val >> XSHIFT;
            *out = data_val;
        }
        state
    }

    /// `generate_state(n64, np.uint64)`: numpy generates `2*n64` u32 words then packs
    /// pairs little-endian into u64 (low word first).
    pub fn generate_state_u64(&self, n64: usize) -> Vec<u64> {
        let words = self.generate_state(n64 * 2);
        (0..n64)
            .map(|i| (words[2 * i] as u64) | ((words[2 * i + 1] as u64) << 32))
            .collect()
    }

    /// The mixed entropy pool (for parity validation against numpy `ss.pool`).
    #[cfg(test)]
    pub fn pool(&self) -> [u32; 4] {
        self.pool
    }
}

/// numpy `_int_to_uint32_array`: little-endian 32-bit words of a non-negative int;
/// at least one word (so 0 -> [0]).
fn int_to_uint32_array(mut v: u128) -> Vec<u32> {
    let mut out = Vec::new();
    if v == 0 {
        return vec![0];
    }
    while v != 0 {
        out.push((v & 0xffff_ffff) as u32);
        v >>= 32;
    }
    out
}

/// numpy SeedSequence.mix_entropy: fill the pool from entropy with INIT_A hashing,
/// then run the pool-mixing rounds. pool_size = 4.
fn mix_entropy(entropy: &[u32]) -> [u32; 4] {
    let pool_size = 4usize;
    let mut pool = [0u32; 4];
    let mut hash_const = INIT_A;

    // Closure mirroring numpy's `hashmix`.
    let hashmix = |value: u32, hc: &mut u32| -> u32 {
        let mut value = value ^ *hc;
        *hc = hc.wrapping_mul(MULT_A);
        value = value.wrapping_mul(*hc);
        value ^= value >> XSHIFT;
        value
    };
    // Closure mirroring numpy's `mix`.
    let mix = |x: u32, y: u32| -> u32 {
        let mut result = MIX_MULT_L.wrapping_mul(x).wrapping_sub(MIX_MULT_R.wrapping_mul(y));
        result ^= result >> XSHIFT;
        result
    };

    // Add in the entropy up to the pool size (zero-pad shorter entropy).
    for i in 0..pool_size {
        if i < entropy.len() {
            pool[i] = hashmix(entropy[i], &mut hash_const);
        } else {
            pool[i] = hashmix(0, &mut hash_const);
        }
    }
    // Mix all bits together so late bits can affect earlier bits.
    for i_src in 0..pool_size {
        for i_dst in 0..pool_size {
            if i_src != i_dst {
                let h = hashmix(pool[i_src], &mut hash_const);
                pool[i_dst] = mix(pool[i_dst], h);
            }
        }
    }
    // Add any remaining entropy, mixing each new word with every pool word.
    for &e in entropy.iter().skip(pool_size) {
        for i_dst in 0..pool_size {
            let h = hashmix(e, &mut hash_const);
            pool[i_dst] = mix(pool[i_dst], h);
        }
    }
    pool
}

/// numpy PCG64 (XSL-RR 128/64). State + increment are 128-bit. Seeded from a
/// SeedSequence's 4 u64 words: `state = words[0..1]`, `inc = words[2..3]`,
/// initialised via the LCG-step bootstrap numpy uses.
pub struct Pcg64 {
    state: u128,
    inc: u128,
}

const PCG_MULT: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;

impl Pcg64 {
    /// `default_rng(int)` -> PCG64(SeedSequence(int)).
    pub fn from_seed_int(seed: u128) -> Self {
        let words = SeedSequence::new(seed).generate_state_u64(4);
        let init_state = ((words[0] as u128) << 64) | (words[1] as u128);
        let init_seq = ((words[2] as u128) << 64) | (words[3] as u128);
        Self::srandom(init_state, init_seq)
    }

    /// pcg_setseq_128_srandom_r: inc = (seq << 1) | 1; state = 0; step; += init_state; step.
    fn srandom(init_state: u128, init_seq: u128) -> Self {
        let inc = (init_seq << 1) | 1;
        let mut rng = Pcg64 { state: 0, inc };
        rng.step();
        rng.state = rng.state.wrapping_add(init_state);
        rng.step();
        rng
    }

    #[inline]
    fn step(&mut self) {
        self.state = self.state.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
    }

    /// pcg_output_xsl_rr_128_64: fold the 128-bit state to 64 bits with a rotate.
    #[inline]
    fn output(state: u128) -> u64 {
        let hi = (state >> 64) as u64;
        let lo = state as u64;
        let rot = (state >> 122) as u32; // top 6 bits
        let xored = hi ^ lo;
        xored.rotate_right(rot)
    }

    /// next 64-bit output (step state, then emit the previous state's output —
    /// matching pcg's "advance then output current" ordering used by numpy).
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.step();
        Pcg64::output(self.state)
    }

    /// numpy `next_double`: 53-bit mantissa in [0, 1).
    #[inline]
    pub fn next_double(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

/// numpy Generator wrapper exposing the three scalar draws volcanic uses.
pub struct Generator {
    rng: Pcg64,
}

impl Generator {
    pub fn from_seed_int(seed: u128) -> Self {
        Generator { rng: Pcg64::from_seed_int(seed) }
    }

    /// `rng.random()` — uniform [0, 1) double.
    #[inline]
    pub fn random(&mut self) -> f64 {
        self.rng.next_double()
    }

    /// `rng.uniform(low, high)` (scalar) = low + (high-low)*next_double.
    #[inline]
    pub fn uniform(&mut self, low: f64, high: f64) -> f64 {
        let range = high - low;
        low + range * self.rng.next_double()
    }

    /// `rng.normal(loc, scale)` (scalar) = loc + scale * standard_normal (Ziggurat).
    #[inline]
    pub fn normal(&mut self, loc: f64, scale: f64) -> f64 {
        loc + scale * self.standard_normal()
    }

    /// numpy `random_standard_normal` — the 256-region Ziggurat (`random_gauss_zig`).
    ///
    /// Bit-exact port of numpy `distributions.c::random_standard_normal`:
    ///   r = next_u64; idx = r & 0xff; r >>= 8;
    ///   sign = r & 1; rabs = (r >> 1) & 0x000f_ffff_ffff_ffff (52 bits);
    ///   x = rabs * wi[idx]; if sign { x = -x };
    ///   if rabs < ki[idx] { return x }
    ///   if idx == 0 { tail: xx = -inv_r*log1p(-U); yy = -log1p(-U);
    ///                 while !(yy+yy > xx*xx) redraw;
    ///                 return ((rabs>>8)&1) ? -(R+xx) : R+xx }
    ///   else { wedge: if (fi[idx-1]-fi[idx])*U + fi[idx] < exp(-0.5*x*x) { return x } }
    fn standard_normal(&mut self) -> f64 {
        let t = ziggurat::tables();
        loop {
            let r0 = self.rng.next_u64();
            let idx = (r0 & 0xff) as usize;
            let r = r0 >> 8;
            let sign = r & 0x1;
            let rabs = (r >> 1) & 0x000f_ffff_ffff_ffff;
            let mut x = (rabs as f64) * t.wi[idx];
            if sign != 0 {
                x = -x;
            }
            if rabs < t.ki[idx] {
                return x;
            }
            if idx == 0 {
                loop {
                    // numpy: -npy_log1p(-next_double()) == (-U).ln_1p().
                    let xx = -ZIGGURAT_NOR_INV_R * (-self.rng.next_double()).ln_1p();
                    let yy = -(-self.rng.next_double()).ln_1p();
                    if yy + yy > xx * xx {
                        return if ((rabs >> 8) & 0x1) != 0 {
                            -(ZIGGURAT_NOR_R + xx)
                        } else {
                            ZIGGURAT_NOR_R + xx
                        };
                    }
                }
            } else {
                let fi_diff = t.fi[idx - 1] - t.fi[idx];
                if fi_diff * self.rng.next_double() + t.fi[idx] < (-0.5 * x * x).exp() {
                    return x;
                }
            }
        }
    }
}

/// numpy ziggurat tail constants for the normal distribution
/// (`ziggurat_nor_r` and `ziggurat_nor_inv_r` from numpy's `ziggurat_constants.h`).
const ZIGGURAT_NOR_R: f64 = 3.6541528853610087963519472518;
const ZIGGURAT_NOR_INV_R: f64 = 0.27366123732975827203338247596;

/// Ziggurat tables matching numpy's `wi_double`/`ki_double`/`fi_double`. Built once
/// at first use from numpy's documented 256-layer construction (the same recurrence
/// numpy's constants-generator uses). The build is validated against numpy's actual
/// `standard_normal` output in `recipes_volcanic_tests.rs` (matches to ~1e-11, which
/// — through the smooth cone/blur pipeline — leaves the final height parity far below
/// the 1e-9 ceiling; see the recipe docstring).
mod ziggurat {
    use super::ZIGGURAT_NOR_R;
    use std::sync::OnceLock;

    pub struct Tables {
        pub ki: [u64; 256],
        pub wi: [f64; 256],
        pub fi: [f64; 256],
    }

    static TABLES: OnceLock<Tables> = OnceLock::new();

    /// 2^52 — the scale numpy uses for the double ziggurat tables (`rabs` is 52 bits).
    const SCALE: f64 = 4_503_599_627_370_496.0;
    /// numpy NOR layer volume constant (from `ziggurat_constants.h` generation).
    const V_NOR: f64 = 0.00492867323399;

    fn build() -> Tables {
        let m = 256usize;
        let r = ZIGGURAT_NOR_R;
        let v = V_NOR;
        let f = |x: f64| (-0.5 * x * x).exp();

        // xs[m] = v / f(r); xs[m-1] = r; xs[i] = sqrt(-2 ln(v/xs[i+1] + f(xs[i+1])))
        let mut xs = [0.0_f64; 257];
        xs[m] = v / f(r);
        xs[m - 1] = r;
        for i in (1..=(m - 2)).rev() {
            xs[i] = (-2.0 * (v / xs[i + 1] + f(xs[i + 1])).ln()).sqrt();
        }
        xs[0] = 0.0;

        let mut wi = [0.0_f64; 256];
        let mut ki = [0u64; 256];
        let mut fi = [0.0_f64; 256];
        for i in 1..m {
            wi[i] = xs[i] / SCALE;
            fi[i] = f(xs[i]);
        }
        wi[0] = v / f(r) / SCALE;
        fi[0] = 1.0;
        ki[0] = ((r * f(r) / v) * SCALE) as u64;
        ki[1] = 0;
        for i in 2..m {
            ki[i] = ((xs[i - 1] / xs[i]) * SCALE) as u64;
        }
        Tables { ki, wi, fi }
    }

    pub fn tables() -> &'static Tables {
        TABLES.get_or_init(build)
    }
}

#[cfg(test)]
mod tests {
    use super::{Generator, Pcg64, SeedSequence};

    // All ground truth captured from numpy 2.4.4 `np.random.default_rng` / SeedSequence.
    #[test]
    fn seed_sequence_pool_and_state_match_numpy() {
        // numpy: SeedSequence(500).pool, .generate_state(4, uint64)
        let ss = SeedSequence::new(500);
        assert_eq!(ss.pool(), [651613600, 3186613311, 1483391812, 1252258596]);
        assert_eq!(
            ss.generate_state_u64(4),
            vec![
                8103700348115910429,
                17905415729801093085,
                18419810088540447113,
                4037860421216475574
            ]
        );
        let ss7 = SeedSequence::new(507);
        assert_eq!(ss7.pool(), [1204273392, 1399742488, 4199502328, 3537793689]);
        let ss9 = SeedSequence::new(900);
        assert_eq!(ss9.pool(), [344492167, 1746722148, 3549973126, 2841617513]);
    }

    #[test]
    fn pcg64_raw_stream_matches_numpy() {
        // numpy: default_rng(500).bit_generator.random_raw() x6
        let mut p = Pcg64::from_seed_int(500);
        let expect = [
            10454565715492221534u64,
            15753113395430921017,
            11904739087875483041,
            7584504502438486222,
            8633665507284389612,
            15151440238235170786,
        ];
        for (k, &e) in expect.iter().enumerate() {
            let got = p.next_u64();
            assert_eq!(got, e, "raw u64 #{k}: got {got} want {e}");
        }
    }

    #[test]
    fn pcg64_random_matches_numpy_seed500() {
        // STYLES[0] vent stream: default_rng(seed+500); seed=0.
        let mut g = Generator::from_seed_int(500);
        let n0 = g.normal(0.0, 4800.0); // span*0.08 = 60000*0.08
        let n1 = g.normal(0.0, 4800.0);
        assert!((n0 - 3350.088848951808).abs() < 1e-7, "n0={n0}");
        assert!((n1 - (-4163.82158704271)).abs() < 1e-7, "n1={n1}");
        // random() draws are bit-exact (no ziggurat).
        let r0 = g.random();
        assert!((r0 - 0.645357199097385).abs() < 1e-15, "r0={r0}");
        let r1 = g.random();
        assert!((r1 - 0.4111568129384948).abs() < 1e-15, "r1={r1}");
        let r2 = g.random();
        assert!((r2 - 0.4680319449755449).abs() < 1e-15, "r2={r2}");
    }

    #[test]
    fn pcg64_uniform_matches_numpy_seed900() {
        use std::f64::consts::PI;
        // STYLES[0] flow-direction stream: default_rng(seed+900); seed=0.
        let mut g = Generator::from_seed_int(900);
        let expect = [-3.10801602613677, -2.8620128338816295, 2.5596769870805165];
        for (k, &e) in expect.iter().enumerate() {
            let d = g.uniform(-PI, PI);
            assert!((d - e).abs() < 1e-13, "uniform #{k}: got {d} want {e}");
        }
    }

    #[test]
    fn standard_normal_sequence_matches_numpy() {
        // numpy: default_rng(12345).standard_normal(8)
        let mut g = Generator::from_seed_int(12345);
        let expect = [
            -1.4238250364546312,
            1.2637284581291104,
            -0.8706617379590857,
            -0.2591732349343976,
            -0.07534330701052097,
            -0.740884652085609,
            -1.3677927017829434,
            0.6488928021930399,
        ];
        let mut maxd = 0.0_f64;
        for (k, &e) in expect.iter().enumerate() {
            let z = g.normal(0.0, 1.0);
            let d = (z - e).abs();
            if d > maxd {
                maxd = d;
            }
            assert!(d < 1e-9, "normal #{k}: got {z} want {e} (|d|={d:.2e})");
        }
        eprintln!("[volcanic ziggurat] standard_normal max |delta| vs numpy = {maxd:.3e}");
    }
}
