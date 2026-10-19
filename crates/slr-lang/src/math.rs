//! `java.lang.Math` natives (M1-06 rows 2..5).
//!
//! Only the ports that have landed are listed here; every other `Math`
//! address stays [`crate::DispatchError::NotPorted`]. The landed set is
//! `randomize` (`LAB_0047c7d0`), `setrandseed` (`FUN_0047c7f0`), `random`
//! (`LAB_0047c820`) and `sqrt` (`LAB_0047c850`).

use slr_platform::seed_current_thread;
use slr_vm::interp::SCRIPT_PRECISION;
use slr_vm::values::Value;
use slr_x87::{Rounding, X87};

use crate::seam::{LangHost, NativeResult, NativeTable};

/// The game's x87 rounding control: nearest-even (FPCW RC=0, ADR 0007).
const RC: Rounding = Rounding::NearestEven;

/// The `random()` multiplier at `0x5f1388` (`FMUL dword ptr [0x5f1388]`).
///
/// Its bits are `0x3800_0100`, i.e. `2⁻¹⁵ · (1 + 2⁻¹⁵)` = 3.0518509e-05.
/// That is the *rounded-up* f32 neighbour of `1/32768`; the handoff note's
/// "f32 `1/32768`" is this value's rounded decimal, not `0x3800_0000`
/// (`2⁻¹⁵` exactly, a different and too-small constant). The extra `2⁻³⁰`
/// makes the product need a real 24-bit rounding for 15-bit draws.
const RANDOM_SCALE: u32 = 0x3800_0100;

/// `java.lang.Math.randomize()V`
///
/// exe: LAB_0047c7d0
///
/// ```text
/// 47c7d0 call FUN_005516c0      ; clock(): ST0 = (QPC - prev) / QPF * 1000.0f
/// 47c7d5 call FUN_005d6750      ; __ftol(ST0) -> eax, truncating toward zero
/// 47c7da push eax
/// 47c7db call FUN_005d73fb      ; srand(eax)
/// 47c7e0 pop  ecx
/// 47c7e1 ret                    ; V: the caller pushes no return cell
/// ```
///
/// The native takes no parameters, so the marshaller is never entered and
/// `args` is untouched.
///
/// `clock()` reaches the port through [`LangHost::read_clock`], which returns
/// the whole elapsed-milliseconds reading — the exe's `(QPC - prev) / QPF *
/// 1000.0f` with constant `0x5f0910` = f32 `1000.0` — and stores it in
/// [`LangHost::clock`].
///
/// The seed is `__ftol` of that reading. `FUN_005d6750` saves the FPCW and
/// installs RC=chop, so the conversion truncates toward zero whatever the
/// caller's rounding control; `FISTP` ignores PC as well. A reading outside
/// the `i32` range (or NaN/infinity) is the indefinite `0x8000_0000`, exactly
/// as the exe's `__ftol` yields it. `srand` then stores those bits in the CRT
/// `rand` word, so both mirrors of `[ptd+0x14]` update:
/// [`LangHost::rng`] and [`slr_platform::seed_current_thread`].
pub fn randomize(host: &mut LangHost, _args: &[Value]) -> NativeResult {
    let seed = X87::from_f32(host.read_clock()).to_i32(Rounding::Zero) as u32;
    host.rng.reseed(seed);
    seed_current_thread(seed);
    None
}

/// The first declared `I` parameter of a native call, read off the argument
/// container exactly as `FUN_0045d910` walks it: the receiver/return slot is
/// `args[len-1]`, so the first parameter is `args[len-2]`. A container with
/// no slot left is the marshaller's `NotEnoughParameters`; `setrandseed`'s
/// out word is zero-initialized before the call (`0x47c7fd`), so the seed is
/// `0` in that case.
///
/// The word is the cell payload verbatim: the marshaller copies `I` cells
/// without inspecting their tag, so `-1` is the seed `0xffff_ffff`.
fn first_int(args: &[Value]) -> u32 {
    match args.len().checked_sub(2) {
        Some(index) => args[index].payload,
        None => 0,
    }
}

/// `java.lang.Math.setrandseed(I)V`
///
/// exe: FUN_0047c7f0
///
/// ```text
/// 47c7f0 push ecx
/// 47c7f1 mov  ecx, [esp+8]      ; CallInfo
/// 47c7f5 lea  eax, [esp]        ; out word
/// 47c7f9 push eax
/// 47c7fa push 0                 ; no receiver out
/// 47c7fc push ecx
/// 47c7fd mov  dword ptr [esp+0xc], 0
/// 47c805 call FUN_0045d910      ; marshal (I)V
/// 47c80a mov  edx, [esp+0xc]    ; seed
/// 47c80e push edx
/// 47c80f call FUN_005d73fb      ; srand(seed)
/// ```
///
/// `srand` only stores the seed in the CRT `rand` word at `[ptd+0x14]`
/// (`FUN_005d73fb`), which both mirrors of that word update:
/// [`LangHost::rng`] and [`slr_platform::seed_current_thread`].
pub fn setrandseed(host: &mut LangHost, args: &[Value]) -> NativeResult {
    let seed = first_int(args);
    host.rng.reseed(seed);
    seed_current_thread(seed);
    None
}

/// `java.lang.Math.random()F`
///
/// exe: LAB_0047c820
///
/// ```text
/// 47c820 push ecx
/// 47c821 call FUN_005d7408      ; rand(): EAX = bits 16..=30 of the new state
/// 47c826 and  eax, 0x7fff       ; draw = rand() & 0x7fff
/// 47c82b cmp  eax, 0x7fff
/// 47c830 mov  [esp], eax        ; stash the draw
/// 47c834 jne  0x47c83e
/// 47c836 mov  [esp], 0x7ffe     ; a full-scale draw is pulled down one step
/// 47c83e fild dword ptr [esp]   ; ST0 = draw (signed load; draw <= 0x7ffe)
/// 47c842 fmul dword ptr [0x5f1388] ; ST0 *= RANDOM_SCALE at PC=24 / RC=0
/// 47c848 pop  ecx
/// 47c849 ret                    ; ST0 = the draw on [0, 1)
/// ```
///
/// `rand()` is [`MsvcRand::next_u15`] (`FUN_005d7408`): it returns
/// `(state >> 16) & 0x7fff`, so the exe's `and eax, 0x7fff` clears nothing
/// and the draw is the returned word.
///
/// A draw of `0x7fff` is replaced by `0x7ffe`. With [`RANDOM_SCALE`] slightly
/// above `1/32768`, `32767 · scale` is still below `1.0`, but the exe excludes
/// it all the same: the native's range is `[0, 32766 · scale]`, i.e. no draw
/// ever reaches the top of the interval.
///
/// `FILD` is exact, and `FMUL` at the game's PC=24 (`SCRIPT_PRECISION`) / RC=0
/// rounds the exact product to a 24-bit significand. The caller's `F` return
/// path stores `ST0` as an `f32` under the same control word, which for this
/// result is exact (24-bit significand, ordinary exponent). This port runs the
/// product through [`X87`] at the same `(PC, RC)`, so it returns the identical
/// payload; the machine code word above is what pins the constant and the
/// clamp.
///
/// `random` reads no argument (`()F`), so `args` is ignored: the exe's native
/// never touches the `CallInfo`.
pub fn random(host: &mut LangHost, _args: &[Value]) -> NativeResult {
    let mut draw = u32::from(host.rng.next_u15());
    if draw == 0x7fff {
        draw = 0x7ffe;
    }
    let scaled = X87::from_i32(draw as i32).mul(
        X87::from_f32(f32::from_bits(RANDOM_SCALE)),
        SCRIPT_PRECISION,
        RC,
    );
    Some(scaled.to_f32(SCRIPT_PRECISION, RC).to_bits())
}

/// The first declared `F` parameter of a native call, read off the argument
/// container exactly like [`first_int`]: the marshaller copies the cell's
/// payload word without inspecting its tag, so the bits are the parameter
/// whatever tag the cell carries. A container with no slot left yields
/// `0.0`; the exe's out slot is the uninitialized `CallInfo` argument in that
/// path (it has no `setrandseed`-style zeroing), and the VM cannot build
/// such a call.
fn first_float(args: &[Value]) -> f32 {
    match args.len().checked_sub(2) {
        Some(index) => args[index].as_f32(),
        None => 0.0,
    }
}

/// `java.lang.Math.sqrt(F)F`
///
/// exe: LAB_0047c850
///
/// ```text
/// 47c850 mov  ecx, [esp+4]      ; CallInfo
/// 47c854 lea  eax, [esp+4]      ; out slot for the (F)F parameter
/// 47c858 push eax
/// 47c859 push 0                 ; no receiver out
/// 47c85b push ecx
/// 47c85c call FUN_0045d910      ; marshal: out[0] = args[count-2]
/// 47c861 fld  dword [esp+0x10]  ; FLD m32: the parameter, exactly
/// 47c865 fsqrt
/// 47c867 add  esp, 0xc
/// 47c86a ret                    ; ST0 = the root
/// ```
///
/// `FLD m32` widens the operand exactly and `FSQRT` rounds under the FPCW;
/// the game runs PC=24 / RC=0 (`SCRIPT_PRECISION`), and both x87 and the
/// M2-10 oracle honour PC on `FSQRT`. The caller's `F` return path then
/// stores `ST0` as an `f32` under the same control word, so the payload is
/// the correctly rounded `f32` square root. `sqrt` of a negative operand is
/// the indefinite QNaN (`0xffc0_0000`), of `-0.0` is `-0.0`, and a NaN
/// operand propagates (a signaling NaN is quieted).
pub fn sqrt(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let input = X87::from_f32(first_float(args));
    let root = input.sqrt(SCRIPT_PRECISION, RC);
    Some(root.to_f32(SCRIPT_PRECISION, RC).to_bits())
}

/// Install the `java.lang.Math` ports into a dispatch table.
pub fn install(table: &mut NativeTable) {
    /// `java.lang.Math.randomize`
    const RANDOMIZE_ADDR: u32 = 0x0047_c7d0;
    table.set(RANDOMIZE_ADDR, randomize);

    /// `java.lang.Math.setrandseed`
    const IMPL_ADDR: u32 = 0x0047_c7f0;
    table.set(IMPL_ADDR, setrandseed);

    /// `java.lang.Math.random`
    const RANDOM_ADDR: u32 = 0x0047_c820;
    table.set(RANDOM_ADDR, random);

    /// `java.lang.Math.sqrt`
    const SQRT_ADDR: u32 = 0x0047_c850;
    table.set(SQRT_ADDR, sqrt);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seam::{self, DispatchError};
    use slr_platform::{current_thread_rng, MsvcRand};

    /// The `(I)V` container the VM builds: parameter first, receiver/return
    /// slot last.
    fn call_args(seed: i32) -> [Value; 2] {
        [Value::int(seed), Value::int(0)]
    }

    fn call(table: &NativeTable, host: &mut LangHost, args: &[Value]) -> NativeResult {
        table
            .dispatch(host, 0x0047_c7f0, args)
            .expect("setrandseed is ported")
    }

    /// The native is `V`: no return cell, and the seed lands in the host's
    /// CRT word unchanged. The following draws are the LCG's own sequence
    /// for that seed.
    #[test]
    fn seeds_the_crt_rand_word() {
        let mut table = NativeTable::new();
        install(&mut table);
        assert_eq!(
            table.ported(),
            vec![0x0047_c7d0, 0x0047_c7f0, 0x0047_c820, 0x0047_c850]
        );

        let mut host = LangHost::new();
        assert_eq!(host.rng.state(), 1, "fresh thread word");
        let args = call_args(1234);
        assert_eq!(call(&table, &mut host, &args), None);
        assert_eq!(host.rng.state(), 1234);

        let mut expected = MsvcRand::seeded(1234);
        for _ in 0..4 {
            assert_eq!(host.rng.next_u15(), expected.next_u15());
        }
    }

    /// Independent anchor for the state word: `srand(1)` is the well-known
    /// VC6 sequence 41, 18467, …
    #[test]
    fn srand_one_yields_the_vc6_sequence() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        let args = call_args(1);
        assert_eq!(call(&table, &mut host, &args), None);
        assert_eq!(host.rng.next_u15(), 41);
        assert_eq!(host.rng.next_u15(), 18467);
    }

    /// The marshaller copies the payload word, so a negative script int is
    /// its two's-complement bits, not a clamped seed.
    #[test]
    fn negative_seed_is_stored_as_bits() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        let args = call_args(-1);
        assert_eq!(call(&table, &mut host, &args), None);
        assert_eq!(host.rng.state(), 0xffff_ffff);
        let mut expected = MsvcRand::seeded(0xffff_ffff);
        assert_eq!(host.rng.next_u15(), expected.next_u15());
    }

    /// A later call overwrites the word (the exe's `srand` is a bare store,
    /// with no "already seeded" state).
    #[test]
    fn reseeding_overwrites() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        for seed in [9, 0, 0x7fff_ffff] {
            let args = call_args(seed);
            assert_eq!(call(&table, &mut host, &args), None);
            assert_eq!(host.rng.state(), seed as u32);
        }
        let mut expected = MsvcRand::seeded(0x7fff_ffff);
        assert_eq!(host.rng.next_u15(), expected.next_u15());
    }

    /// The exe zero-initializes the out word before marshalling, so a
    /// container too short for the parameter still seeds `0` rather than
    /// failing or leaving the previous seed in place.
    #[test]
    fn a_missing_parameter_seeds_zero() {
        let mut table = NativeTable::new();
        install(&mut table);

        let mut host = LangHost::new();
        host.rng.reseed(99);
        assert_eq!(call(&table, &mut host, &[]), None);
        assert_eq!(host.rng.state(), 0);

        let mut host = LangHost::new();
        host.rng.reseed(99);
        let receiver_only = [Value::int(0)];
        assert_eq!(call(&table, &mut host, &receiver_only), None);
        assert_eq!(host.rng.state(), 0);
    }

    /// Both mirrors of `[ptd+0x14]` see the same seed.
    #[test]
    fn the_platform_thread_word_is_seeded_too() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        let args = call_args(77);
        assert_eq!(call(&table, &mut host, &args), None);
        let mut expected = MsvcRand::seeded(77);
        assert_eq!(current_thread_rng(), expected.next_u15());
    }

    /// Dispatch `java.lang.Math.random()F` and return the payload word.
    fn random_bits(table: &NativeTable, host: &mut LangHost) -> u32 {
        table
            .dispatch(host, 0x0047_c820, &[])
            .expect("random is ported")
            .expect("an F native returns a payload")
    }

    /// `n` draws from a fresh host seeded with `seed`.
    fn draws(table: &NativeTable, seed: u32, n: usize) -> Vec<u32> {
        let mut host = LangHost::new();
        host.rng.reseed(seed);
        (0..n).map(|_| random_bits(table, &mut host)).collect()
    }

    /// The exe's own `srand` + `random` output, read from the Unicorn oracle
    /// at cw `0x7f` (the game's PC=24/RC=0): `srand` at `0x5d73fb`, `random`
    /// at `0x47c820`, eight draws per seed. `0x7fff` never appears in these
    /// runs, so every word is `f32(draw · RANDOM_SCALE)`.
    #[test]
    fn random_draws_match_the_exe_sequence() {
        let mut table = NativeTable::new();
        install(&mut table);
        for (seed, expected) in [
            (
                1u32,
                [
                    0x3aa4_0148,
                    0x3f10_4721,
                    0x3e45_f18c,
                    0x3f4f_099e,
                    0x3f15_c32c,
                    0x3ef5_b1eb,
                    0x3eb3_5967,
                    0x3f65_5dcb,
                ],
            ),
            (
                1234,
                [
                    0x3dfe_41fc,
                    0x3bd5_01aa,
                    0x3ec7_658f,
                    0x3e88_d912,
                    0x3f34_2168,
                    0x3e71_29e2,
                    0x3eee_a9dd,
                    0x3f3f_797f,
                ],
            ),
            (
                0xffff_ffff,
                [
                    0x3a8c_0118,
                    0x3f68_57d1,
                    0x3dd2_e1a6,
                    0x3eae_155c,
                    0x3f74_99e9,
                    0x3e75_f1ec,
                    0x3e24_2948,
                    0x3d98_d132,
                ],
            ),
        ] {
            assert_eq!(
                draws(&table, seed, expected.len()),
                expected.to_vec(),
                "seed {seed:#x}"
            );
        }
    }

    /// A draw of `0x7fff` is pulled down to `0x7ffe` (`0x47c82b`..`0x47c836`).
    /// Seed `0xf01bf641` makes the first draw exactly `0x7fff`, and the exe
    /// returns `32766 · RANDOM_SCALE` (`0x3f7ffe00`), not `32767 ·` it.
    #[test]
    fn a_full_scale_draw_is_pulled_down_to_0x7ffe() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut expected = MsvcRand::seeded(0xf01b_f641);
        assert_eq!(expected.next_u15(), 0x7fff, "seed chosen for a full draw");

        let seq = draws(&table, 0xf01b_f641, 8);
        let scale = f32::from_bits(RANDOM_SCALE);
        assert_eq!(seq[0], 0x3f7f_fe00);
        assert_eq!(f32::from_bits(seq[0]), 32766.0 * scale);
        assert_ne!(f32::from_bits(seq[0]), 32767.0 * scale);

        assert_eq!(
            seq,
            vec![
                0x3f7f_fe00,
                0x3ef0_a5e1,
                0x3ed8_79b1,
                0x3e9c_4539,
                0x3ee4_d1ca,
                0x3df8_a1f1,
                0x3f66_79cd,
                0x3eb4_e16a,
            ]
        );
    }

    /// The returned word is the single-rounded `f32(draw · RANDOM_SCALE)` of
    /// the LCG draw: `FILD` is exact and `FMUL` at PC=24 rounds once, exactly
    /// like an `f32` multiply. A 256-draw sweep pins it beyond the fixed
    /// sequences above.
    #[test]
    fn draws_are_the_rounded_product_of_the_lcg_word() {
        let mut table = NativeTable::new();
        install(&mut table);
        let scale = f32::from_bits(RANDOM_SCALE);
        let mut expected = MsvcRand::seeded(0x1234_5678);
        let mut host = LangHost::new();
        host.rng.reseed(0x1234_5678);
        for draw_index in 0..256 {
            let mut draw = u32::from(expected.next_u15());
            if draw == 0x7fff {
                draw = 0x7ffe;
            }
            assert_eq!(
                random_bits(&table, &mut host),
                (draw as f32 * scale).to_bits(),
                "draw {draw_index}"
            );
        }
    }

    /// `random` reads no `CallInfo` (`()F`): the argument container is
    /// ignored, and each call consumes exactly one `rand()` draw.
    #[test]
    fn random_ignores_its_arguments_and_consumes_one_draw() {
        let mut table = NativeTable::new();
        install(&mut table);

        let extra = [Value::int(0), Value::float(1.0)];
        let mut empty = LangHost::new();
        empty.rng.reseed(1);
        let mut slotted = LangHost::new();
        slotted.rng.reseed(1);
        let a = table.dispatch(&mut empty, 0x0047_c820, &[]);
        let b = table.dispatch(&mut slotted, 0x0047_c820, &extra);
        assert_eq!(a, b);
        assert_eq!(empty.rng.state(), slotted.rng.state());

        let mut host = LangHost::new();
        host.rng.reseed(7);
        let mut expected = MsvcRand::seeded(7);
        for _ in 0..5 {
            expected.next_u15();
            random_bits(&table, &mut host);
            assert_eq!(host.rng.state(), expected.state());
        }
    }

    /// Through the thread-local seam: the seed lands via `setrandseed` and the
    /// next `random` returns the exe's first draw for it.
    #[test]
    fn random_is_registered_through_the_seam_table() {
        seam::reset();
        seam::with_native_table(install);
        assert_eq!(seam::dispatch(0x0047_c7f0, &call_args(1)), Ok(None));
        assert_eq!(seam::dispatch(0x0047_c820, &[]), Ok(Some(0x3aa4_0148)));
        seam::with_lang_host(|host| assert_eq!(host.last_error, None));
        seam::reset();
    }

    /// Dispatch `java.lang.Math.randomize()V` with `clock` as the reading the
    /// clock seam hands out, and return the CRT word it seeded.
    fn randomize_seed(table: &NativeTable, host: &mut LangHost, clock: f32) -> u32 {
        host.clock = clock;
        assert_eq!(
            table.dispatch(host, 0x0047_c7d0, &[]),
            Ok(None),
            "randomize is a V native"
        );
        host.rng.state()
    }

    /// The seed is the truncated clock reading, and the following draws are
    /// the LCG's own sequence for it.
    #[test]
    fn randomize_seeds_with_the_truncated_clock_reading() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        // `__ftol` truncates: 1234.9 -> 1234, not the nearest 1235.
        assert_eq!(randomize_seed(&table, &mut host, 1234.9), 1234);
        let mut expected = MsvcRand::seeded(1234);
        for _ in 0..4 {
            assert_eq!(host.rng.next_u15(), expected.next_u15());
        }
        // An hour of elapsed time is still well inside the i32 range.
        assert_eq!(randomize_seed(&table, &mut host, 3_600_000.0), 3_600_000);
    }

    /// `__ftol` installs RC=chop on a saved control word, so a negative
    /// reading truncates toward zero: nearest-even would give -4 here.
    #[test]
    fn randomize_truncates_toward_zero() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        assert_eq!(randomize_seed(&table, &mut host, -3.75), 0xffff_fffd);
        assert_eq!(randomize_seed(&table, &mut host, -0.5), 0);
    }

    /// A reading the `i32` store cannot hold converts to the indefinite
    /// `0x8000_0000` as the exe's `__ftol` does, never to a saturated
    /// maximum.
    #[test]
    fn randomize_of_an_unrepresentable_reading_seeds_the_indefinite_word() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        assert_eq!(randomize_seed(&table, &mut host, 3.0e9), 0x8000_0000);
        assert_eq!(randomize_seed(&table, &mut host, f32::NAN), 0x8000_0000);
        assert_eq!(
            randomize_seed(&table, &mut host, f32::INFINITY),
            0x8000_0000
        );
    }

    /// Without a source the seam replays the stored reading; with one the
    /// source owns the reading. Either way the call runs: a fresh host seeds
    /// `0` rather than being skipped.
    #[test]
    fn randomize_reads_the_clock_seam() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        assert_eq!(randomize_seed(&table, &mut host, 0.0), 0);
        assert_eq!(randomize_seed(&table, &mut host, 41.5), 41);

        let mut replayed = LangHost::with_clock(|| 2048.75);
        assert_eq!(randomize_seed(&table, &mut replayed, 0.0), 2048);
        assert_eq!(replayed.clock, 2048.75, "the sample is stored");
    }

    /// `randomize` reaches the same `srand` as `setrandseed`, so the
    /// platform's thread word sees the seed too.
    #[test]
    fn randomize_mirrors_the_platform_thread_word() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        assert_eq!(randomize_seed(&table, &mut host, 77.9), 77);
        let mut expected = MsvcRand::seeded(77);
        assert_eq!(current_thread_rng(), expected.next_u15());
    }

    /// Dispatch `java.lang.Math.sqrt(F)F` with an `f32` operand encoded as
    /// raw bits and return the payload word.
    fn sqrt_bits(table: &NativeTable, host: &mut LangHost, input: u32) -> u32 {
        let args = [Value::float(f32::from_bits(input)), Value::int(0)];
        table
            .dispatch(host, 0x0047_c850, &args)
            .expect("sqrt is ported")
            .expect("an F native returns a payload")
    }

    /// The correctly rounded `f32` roots (reference: C `sqrtf`).
    #[test]
    fn sqrt_of_common_operands() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        for (input, expected) in [
            (0x4080_0000, 0x4000_0000), // 4.0 -> 2.0
            (0x4000_0000, 0x3fb5_04f3), // 2.0 -> 1.4142135
            (0x3e80_0000, 0x3f00_0000), // 0.25 -> 0.5
            (0x4110_0000, 0x4040_0000), // 9.0 -> 3.0
            (0x40e8_0000, 0x402c_5345), // 7.25 -> 2.6925824
        ] {
            assert_eq!(
                sqrt_bits(&table, &mut host, input),
                expected,
                "sqrt(0x{input:08x})"
            );
        }
    }

    /// Denormals and the extreme finite operands keep the full `f32`
    /// exponent range: `FSQRT` rounds the significand at PC=24 but the
    /// exponent is untouched, and the `F` return stores the value exactly
    /// (every root is normal or zero).
    #[test]
    fn sqrt_of_denormals_and_extremes() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        for (input, expected) in [
            (0x0000_0001, 0x1a35_04f3), // min denormal
            (0x0080_0000, 0x2000_0000), // min normal
            (0x7f7f_ffff, 0x5f7f_ffff), // max finite
            (0x7149_f2ca, 0x5863_5fa9), // 1e30
        ] {
            assert_eq!(
                sqrt_bits(&table, &mut host, input),
                expected,
                "sqrt(0x{input:08x})"
            );
        }
    }

    /// `FSQRT` of a negative operand raises `#IA` and yields the indefinite
    /// QNaN (sign set), not a positive NaN.
    #[test]
    fn a_negative_operand_yields_the_indefinite_qnan() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        for input in [0xbf80_0000, 0xc080_0000, 0xff80_0000] {
            assert_eq!(
                sqrt_bits(&table, &mut host, input),
                0xffc0_0000,
                "sqrt(0x{input:08x})"
            );
        }
    }

    /// Signed zero and infinity are exact.
    #[test]
    fn sqrt_of_zero_and_infinity() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        assert_eq!(sqrt_bits(&table, &mut host, 0x0000_0000), 0x0000_0000);
        assert_eq!(sqrt_bits(&table, &mut host, 0x8000_0000), 0x8000_0000);
        assert_eq!(sqrt_bits(&table, &mut host, 0x7f80_0000), 0x7f80_0000);
    }

    /// A quiet NaN propagates with its payload; a signaling NaN is quieted
    /// (quiet bit set, payload kept) and raises `#IA`.
    #[test]
    fn nan_operands_follow_x87_propagation() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        assert_eq!(sqrt_bits(&table, &mut host, 0x7fc0_1234), 0x7fc0_1234);
        assert_eq!(sqrt_bits(&table, &mut host, 0x7f80_0001), 0x7fc0_0001);
    }

    /// The marshaller copies the parameter cell's payload word without
    /// inspecting its tag, so an `I` cell carrying the float bits behaves
    /// exactly like an `F` cell.
    #[test]
    fn the_parameter_is_an_unchecked_payload_word() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        let args = [Value::int(0x4080_0000u32 as i32), Value::int(0)];
        assert_eq!(
            table.dispatch(&mut host, 0x0047_c850, &args),
            Ok(Some(0x4000_0000))
        );
    }

    /// A container too short for the parameter reads `0.0`: the exe's out
    /// slot is its uninitialized stack argument in that path (unlike
    /// `setrandseed`, there is no zeroing store), and the VM's argument
    /// collector cannot build such a call.
    #[test]
    fn a_missing_parameter_reads_zero() {
        let mut table = NativeTable::new();
        install(&mut table);
        let mut host = LangHost::new();
        assert_eq!(table.dispatch(&mut host, 0x0047_c850, &[]), Ok(Some(0)));
        assert_eq!(
            table.dispatch(&mut host, 0x0047_c850, &[Value::int(0)]),
            Ok(Some(0))
        );
    }

    /// Registration goes through the seam's table, so the thread-local
    /// dispatch path (`with_native_table` + `dispatch`) reaches the port and
    /// every other `Math` address stays `NotPorted`.
    #[test]
    fn the_port_is_registered_through_the_seam_table() {
        seam::reset();
        seam::with_native_table(install);
        let args = call_args(5);
        assert_eq!(seam::dispatch(0x0047_c7f0, &args), Ok(None));
        seam::with_lang_host(|host| assert_eq!(host.rng.state(), 5));
        assert_eq!(
            seam::dispatch(0x0047_c850, &[Value::float(4.0), Value::int(0)]),
            Ok(Some(0x4000_0000))
        );
        // `randomize` is ported too; the fresh thread host has no clock
        // source, so it seeds the stored reading (0).
        assert_eq!(seam::dispatch(0x0047_c7d0, &[]), Ok(None));
        seam::with_lang_host(|host| assert_eq!(host.rng.state(), 0));
        assert_eq!(
            seam::dispatch(0x0048_1e30, &[]),
            Err(DispatchError::NotPorted {
                impl_addr: 0x0048_1e30
            })
        );
        seam::reset();
        assert_eq!(
            seam::dispatch(0x0047_c7f0, &args),
            Err(DispatchError::NotPorted {
                impl_addr: 0x0047_c7f0
            })
        );
    }

    /// End to end through the VM: `install` puts the bridge at the
    /// registered `record+8` addresses, and calls resolved from
    /// `java.lang.Math` reach the ports.
    #[test]
    fn the_vm_bridge_reaches_the_port() {
        use slr_vm::host::{CallInfo, VmHost};

        seam::reset();
        let mut table = NativeTable::new();
        install(&mut table);
        seam::with_native_table(install);
        let mut vm = VmHost::new();
        seam::install(&mut vm, &table);
        let mut bridged: Vec<u32> = vm.native_impls.keys().copied().collect();
        bridged.sort_unstable();
        assert_eq!(
            bridged,
            vec![0x0047_c7d0, 0x0047_c7f0, 0x0047_c820, 0x0047_c850],
            "only the ported addresses get a bridge"
        );

        let args = call_args(31337);
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: "java.lang.Math",
            name: "setrandseed",
            sig: "(I)V",
        };
        assert_eq!(
            vm.natives
                .resolve(info.class, info.name, info.sig)
                .expect("registered"),
            0x0047_c7f0
        );
        assert_eq!(seam::bridge(&mut vm, &info), None);
        seam::with_lang_host(|host| {
            assert_eq!(host.last_error, None);
            assert_eq!(host.rng.state(), 31337);
        });

        let draw_info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &[],
            class: "java.lang.Math",
            name: "random",
            sig: "()F",
        };
        assert_eq!(
            vm.natives
                .resolve(draw_info.class, draw_info.name, draw_info.sig)
                .expect("registered"),
            0x0047_c820
        );
        // The exe's first draw for seed 31337.
        assert_eq!(seam::bridge(&mut vm, &draw_info), Some(0x3dfe_41fc));
        seam::with_lang_host(|host| assert_eq!(host.last_error, None));

        let sqrt_args = [Value::float(9.0), Value::int(0)];
        let sqrt_info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &sqrt_args,
            class: "java.lang.Math",
            name: "sqrt",
            sig: "(F)F",
        };
        assert_eq!(
            vm.natives
                .resolve(sqrt_info.class, sqrt_info.name, sqrt_info.sig)
                .expect("registered"),
            0x0047_c850
        );
        assert_eq!(seam::bridge(&mut vm, &sqrt_info), Some(0x4040_0000));
        seam::with_lang_host(|host| assert_eq!(host.last_error, None));

        let rand_info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &[],
            class: "java.lang.Math",
            name: "randomize",
            sig: "()V",
        };
        assert_eq!(
            vm.natives
                .resolve(rand_info.class, rand_info.name, rand_info.sig)
                .expect("registered"),
            0x0047_c7d0
        );
        assert_eq!(seam::bridge(&mut vm, &rand_info), None);
        seam::with_lang_host(|host| {
            assert_eq!(host.last_error, None);
            assert_eq!(host.rng.state(), 0, "the thread clock reads 0");
        });
        seam::reset();
    }
}
