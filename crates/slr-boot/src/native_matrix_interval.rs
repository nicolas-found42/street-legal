//! Certify54ecd0's memory-only consumers without selecting unknown extF80
//! transcendental bits. Scalar results/status remain outside this certificate.
//!
//! Intel SDM Vol1 (253665-078US),8.3.10, printed8-22: reduced FSIN/FCOS
//! operands have less than one extF80 ulp error in nearest-even. Restricting
//! |x|<=9/8 is inside BOTH stated safe reduction domains. Directed extF80
//! arithmetic encloses the alternating Taylor series; widening by twice the
//! largest possible true-result ulp includes the allowed hardware error,
//! including binade boundaries. There is no comparison tolerance: a store is
//! admitted only when every enclosed value rounds to the SAME m32 word.
//! CPU status flags and the scalar instruction results are not certified here.
//!
//! For 9/8<|x|<=8, the standalone algorithm pinned at Bochs commit
//! d245c751e6a6fab4bf68e72823598a48001adb3c has a separate rational error
//! proof: exact66-bitPi reduction, coefficient/table deviations from Taylor,
//! and every64/67-bit chop/round in its arithmetic DAG give <1.845e-18 absolute
//! error. A conservative2^-56 enclosure admits only unique m32 consumer stores.
//! This does not extrapolate the SDM's reduced-input bound to unreduced inputs,
//! supply raw scalar bits, or establish original hardware/model identity (G11).
//! No Bochs/GPL oracle code is linked; this module evaluates a directed Taylor
//! enclosure and the existing BSD SoftFloat arithmetic only. The independent
//! proof producer is re/x87probe/prove_model_error.py. The named Unicorn2.1.4
//! macOS oracle narrows to binary64 and calls system sin/cos, then widens exactly.
//! We take the HULL with that independently computed provider point, not an
//! unproved host-libm accuracy bound. Both models must give the same consumer
//! stores. Full raw-point equality tests bind this provider to the oracle. It
//! supplies no scalar x87 result or status to callers, and is never used to
//! choose among ambiguous stores.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};
use std::cmp::Ordering;

const EXT: Precision = Precision::Ext;
const PC: Precision = Precision::F32;
const RN: Rounding = Rounding::NearestEven;

fn neg(value: X87) -> X87 {
    let mut image = value.to_bytes();
    image[9] ^= 0x80;
    X87::from_bytes(image)
}
fn compare(a: X87, b: X87) -> Ordering {
    let (flags, _) = a.fcom(b);
    assert!(!flags.c2, "finite certified arithmetic became unordered");
    if flags.c3 {
        // Preserve both zero signs in an enclosure; they are distinct stores.
        b.sign().cmp(&a.sign())
    } else if flags.c0 {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

#[derive(Clone, Copy)]
struct Interval {
    lo: X87,
    hi: X87,
}
impl Interval {
    fn point(value: X87) -> Self {
        Self {
            lo: value,
            hi: value,
        }
    }
    fn include(self, point: X87) -> Self {
        Self {
            lo: if compare(point, self.lo).is_lt() {
                point
            } else {
                self.lo
            },
            hi: if compare(point, self.hi).is_gt() {
                point
            } else {
                self.hi
            },
        }
    }
    #[cfg(test)]
    fn contains(self, point: X87) -> bool {
        !compare(point, self.lo).is_lt() && !compare(point, self.hi).is_gt()
    }
    fn bits(bits: u32) -> Self {
        Self::point(X87::from_f32(f32::from_bits(bits)))
    }
    fn neg(self) -> Self {
        Self {
            lo: neg(self.hi),
            hi: neg(self.lo),
        }
    }
    fn add_directed(self, rhs: Self) -> Self {
        Self {
            lo: self.lo.add(rhs.lo, EXT, Rounding::Down),
            hi: self.hi.add(rhs.hi, EXT, Rounding::Up),
        }
    }
    fn sub_directed(self, rhs: Self) -> Self {
        Self {
            lo: self.lo.sub(rhs.hi, EXT, Rounding::Down),
            hi: self.hi.sub(rhs.lo, EXT, Rounding::Up),
        }
    }
    // Positive operands only in the Taylor recurrence.
    fn mul_positive(self, rhs: Self) -> Self {
        Self {
            lo: self.lo.mul(rhs.lo, EXT, Rounding::Down),
            hi: self.hi.mul(rhs.hi, EXT, Rounding::Up),
        }
    }
    fn divide_positive(self, divisor: i32) -> Self {
        let d = X87::from_i32(divisor);
        Self {
            lo: self.lo.div(d, EXT, Rounding::Down),
            hi: self.hi.div(d, EXT, Rounding::Up),
        }
    }
    fn operation(self, rhs: Self, op: fn(X87, X87, Precision, Rounding) -> X87) -> Self {
        let mut values = [
            op(self.lo, rhs.lo, PC, RN),
            op(self.lo, rhs.hi, PC, RN),
            op(self.hi, rhs.lo, PC, RN),
            op(self.hi, rhs.hi, PC, RN),
        ];
        values.sort_by(|a, b| compare(*a, *b));
        Self {
            lo: values[0],
            hi: values[3],
        }
    }
    fn mul(self, rhs: Self) -> Self {
        self.operation(rhs, X87::mul)
    }
    fn add(self, rhs: Self) -> Self {
        self.operation(rhs, X87::add)
    }
    fn sub(self, rhs: Self) -> Self {
        self.operation(rhs, X87::sub)
    }
    fn store(self) -> Result<u32, String> {
        let a = self.lo.to_f32(PC, RN).to_bits();
        let b = self.hi.to_f32(PC, RN).to_bits();
        if a == b {
            Ok(a)
        } else {
            Err(format!(
                "NativeMatrixBoundary: inconclusive certified m32 store {a:#010x}..{b:#010x}"
            ))
        }
    }
}

// Unicorn2.1.4's helper_fsin/helper_fcos perform exactly this narrowing,
// system-library operation and exact widening on the permitted macOS oracle.
// Calling the declared host provider is not an assumed libm error tolerance:
// its point is included alongside the independent algorithm enclosure.
fn oracle_provider_points(bits: u32) -> Result<(X87, X87), String> {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            #[link_name = "sin"]
            fn host_sin(value: f64) -> f64;
            #[link_name = "cos"]
            fn host_cos(value: f64) -> f64;
        }
        let value = f32::from_bits(bits) as f64;
        // SAFETY: C sin/cos accept every finite binary64 input. The bounded
        // caller domain excludes NaN/infinity and the narrowing is exact.
        let (sine, cosine) = unsafe { (host_sin(value), host_cos(value)) };
        if !sine.is_finite() || !cosine.is_finite() {
            return Err("NativeMatrixBoundary: declared oracle provider became nonfinite".into());
        }
        Ok((X87::from_f64(sine), X87::from_f64(cosine)))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bits;
        Err("NativeMatrixBoundary: declared macOS oracle provider unavailable".into())
    }
}

fn trig(bits: u32) -> Result<(Interval, Interval), String> {
    let absolute = bits & 0x7fff_ffff;
    if absolute > 0x4100_0000 {
        return Err("NativeMatrixBoundary: certified standalone-model domain exceeded".into());
    }
    let reduced = absolute <= 0x3f90_0000;
    if bits & 0x7fffffff == 0 {
        return Ok((Interval::bits(bits), Interval::bits(0x3f800000)));
    }
    let x = Interval::bits(bits & 0x7fffffff);
    let square = x.mul_positive(x);
    let series = |cosine: bool| {
        let mut term = if cosine {
            Interval::bits(0x3f800000)
        } else {
            x
        };
        let mut sum = term;
        // 32 terms also enclose direct sin/cos throughout the bounded |x|<=8
        // domain. The first omitted term bounds the remaining alternating tail.
        for n in 1..32 {
            let k = 2 * n - i32::from(cosine);
            term = term.mul_positive(square).divide_positive(k * (k + 1));
            sum = if n & 1 == 0 {
                sum.add_directed(term)
            } else {
                sum.sub_directed(term)
            };
        }
        let k = 64 - i32::from(cosine);
        let remainder = term.mul_positive(square).divide_positive(k * (k + 1));
        // The last included term is negative; the true value lies above
        // the partial sum and below partial sum plus the next positive term.
        sum.hi = sum.hi.add(remainder.hi, EXT, Rounding::Up);
        // If E is the exponent of the positive upper enclosure, the true
        // result's extF80 ulp is <=2^(E-63), even when a binade is crossed.
        // Use TWO such ulps and directed endpoints; this is an enclosure of
        // all allowed instruction results, never an equality tolerance.
        let exponent = if reduced {
            sum.hi
                .biased_exp()
                .checked_sub(62)
                .expect("normal reduced result")
        } else {
            // Independent exact-rational proof bounds the pinned standalone
            // algorithm's phase, approximation and arithmetic error by <2^-56.
            // This is an interval premise, never an equality tolerance.
            0x3fff - 56
        };
        let mut image = [0u8; 10];
        image[..8].copy_from_slice(&(1u64 << 63).to_le_bytes());
        image[8..].copy_from_slice(&exponent.to_le_bytes());
        let error = X87::from_bytes(image);
        sum.lo = sum.lo.sub(error, EXT, Rounding::Down);
        sum.hi = sum.hi.add(error, EXT, Rounding::Up);
        sum
    };
    let sine = series(false);
    let (oracle_sine, oracle_cosine) = oracle_provider_points(bits)?;
    Ok((
        (if bits >> 31 != 0 { sine.neg() } else { sine }).include(oracle_sine),
        series(true).include(oracle_cosine),
    ))
}

struct Evaluation<'a> {
    memory: &'a Space,
    writes: Vec<(u32, u32)>,
    stack: Vec<Interval>,
}
impl Evaluation<'_> {
    fn push(&mut self, v: Interval) {
        self.stack.push(v);
    }
    fn get(&self, i: usize) -> Interval {
        self.stack[self.stack.len() - 1 - i]
    }
    fn set(&mut self, i: usize, v: Interval) {
        let p = self.stack.len() - 1 - i;
        self.stack[p] = v;
    }
    fn pop(&mut self) -> Interval {
        self.stack.pop().expect("original x87 stack underflow")
    }
    fn read(&self, p: u32) -> Interval {
        let bits = self
            .writes
            .iter()
            .rev()
            .find(|(a, _)| *a == p)
            .map_or_else(|| self.memory.r32(p), |(_, v)| *v);
        Interval::bits(bits)
    }
    fn load(&mut self, p: u32) {
        self.push(self.read(p));
    }
    fn store(&mut self, p: u32, pop: bool) -> Result<(), String> {
        self.writes.push((p, self.get(0).store()?));
        if pop {
            self.pop();
        }
        Ok(())
    }
    fn mul(&mut self, i: usize) {
        self.set(0, self.get(0).mul(self.get(i)));
    }
    fn mul_memory(&mut self, p: u32) {
        self.set(0, self.get(0).mul(self.read(p)));
    }
    fn add_pop(&mut self) {
        let top = self.pop();
        self.set(0, self.get(0).add(top));
    }
    fn sub_pop(&mut self) {
        let top = self.pop();
        self.set(0, self.get(0).sub(top));
    }
}

/// Original54ecd0 ordered arithmetic with a proof at every observable store.
/// No writes are published if any enclosure is inconclusive or unsupported.
pub fn certify(m: &mut Space, out: u32, angles: u32, frame: u32) -> Result<(), String> {
    let inputs: [u32; 3] = std::array::from_fn(|i| m.r32(angles + i as u32 * 4));
    let [a, b, c] = inputs.map(trig);
    let (sx, cx) = a?;
    let (sy, cy) = b?;
    let (sz, cz) = c?;
    let s = frame.wrapping_sub(0x14);
    let mut e = Evaluation {
        memory: m,
        writes: vec![],
        stack: vec![],
    };
    e.push(sx);
    e.push(cx);
    e.store(s + 0x18, true)?;
    e.push(sy);
    e.push(cy);
    e.store(s + 8, true)?;
    e.push(sz);
    e.store(s + 4, true)?;
    e.push(cz);
    e.store(s, true)?;
    e.load(s + 4);
    e.mul(1);
    e.store(s + 0xc, false)?;
    e.mul(2);
    e.load(s);
    e.mul_memory(s + 0x18);
    e.add_pop();
    e.store(out, true)?;
    e.load(s);
    e.mul(1);
    e.store(s + 0x10, false)?;
    e.mul(2);
    e.load(s + 4);
    e.mul_memory(s + 0x18);
    e.sub_pop();
    e.store(out + 4, true)?;
    e.load(s + 8);
    e.mul(2);
    e.store(out + 8, true)?;
    e.load(s + 4);
    e.mul_memory(s + 8);
    e.store(out + 0x10, true)?;
    e.load(s);
    e.mul_memory(s + 8);
    e.store(out + 0x14, true)?;
    e.set(0, e.get(0).neg());
    e.store(out + 0x18, true)?;
    e.load(s + 0xc);
    e.mul_memory(s + 0x18);
    e.load(s);
    e.mul(2);
    e.sub_pop();
    e.store(out + 0x20, true)?;
    e.load(s + 0x10);
    e.mul_memory(s + 0x18);
    e.load(s + 4);
    e.mul(2);
    e.add_pop();
    e.store(out + 0x24, true)?;
    e.pop();
    e.load(s + 8);
    e.mul_memory(s + 0x18);
    e.store(out + 0x28, true)?;
    assert!(e.stack.is_empty());
    let writes = e.writes;
    for (p, v) in writes {
        m.w32(p, v);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_image(text: &str) -> X87 {
        assert_eq!(text.len(), 20);
        X87::from_bytes(std::array::from_fn(|i| {
            u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap()
        }))
    }

    #[test]
    fn bounded_model_rational_error_proof_is_executable() {
        use sha2::{Digest, Sha256};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let result = std::process::Command::new("python3")
            .arg(root.join("re/x87probe/prove_model_error.py"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let proof: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(proof["schema"], "standalone-model-rational-error-v1");
        assert_eq!(
            proof["source_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("re/x87probe/model/intel_trig.rs")).unwrap()
                )
            )
        );
        assert_eq!(
            proof["producer_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("re/x87probe/prove_model_error.py")).unwrap()
                )
            )
        );
        assert_eq!(proof["results"].as_object().unwrap().len(), 4);
        assert_eq!(
            proof["upstream_pin"],
            "d245c751e6a6fab4bf68e72823598a48001adb3c"
        );
        // The rational proof itself always runs, including fixture-free CI.
        // A retained local receipt is additional provenance: strict mode
        // requires it, and present but unreadable/stale receipts always fail.
        let Some(path) =
            crate::oracle_fixture::file(&root.join("re/out/boot/euler-model-error-proof.json"))
        else {
            return;
        };
        let retained: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(
            retained, proof,
            "regenerate the proof receipt for current tool/source"
        );
    }

    #[test]
    fn every_retained_case_matches_complete_original_memory_and_admitted_raw_points() {
        let required_fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/euler-consumer-original.json");
        if crate::oracle_fixture::file(&required_fixture).is_none() {
            return;
        }

        use sha2::{Digest, Sha256};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("re/out/boot/euler-consumer-original.json");
        let corpus: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(corpus["schema"], "original-euler-complete-corpus-v1");
        assert_eq!(corpus["entry"], 0x54ecd0);
        assert_eq!(corpus["oracle_profile"]["fpu_cw"], 127);
        assert_eq!(corpus["oracle_profile"]["unicorn"], "2.1.4");
        assert_eq!(corpus["oracle_profile"]["substituted_math"], false);
        for (key, file) in [
            (
                "original_sha256",
                "reference/raw/StreetLegal_Redline.bak.exe",
            ),
            ("oracle_sha256", "re/oracle/oracle.py"),
            ("producer_sha256", "re/oracle/euler_consumer_probe.py"),
        ] {
            assert_eq!(
                corpus[key],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(file)).unwrap())
                )
            );
        }
        for (input, digest) in corpus["input_inventories"].as_object().unwrap() {
            assert_eq!(
                digest,
                &serde_json::json!(format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(input)).unwrap())
                )),
                "input inventory changed: {input}"
            );
        }
        let identity = std::process::Command::new("python3")
            .arg(root.join("re/oracle/euler_consumer_probe.py"))
            .arg("--provider-identity")
            .output()
            .unwrap();
        assert!(
            identity.status.success(),
            "{}",
            String::from_utf8_lossy(&identity.stderr)
        );
        let identity: serde_json::Value = serde_json::from_slice(&identity.stdout).unwrap();
        assert_eq!(identity["os_build"], corpus["oracle_profile"]["os_build"]);
        assert_eq!(identity["symbols"], corpus["oracle_profile"]["symbols"]);
        let rows = corpus["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1515);
        let (mut parity, mut unsupported, mut raw_count) = (0, 0, 0);
        for (index, row) in rows.iter().enumerate() {
            let angles: Vec<u32> = row["angles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            let bounded = angles.iter().all(|bits| bits & 0x7fff_ffff <= 0x4100_0000);
            let scalar = angles.iter().all(|bits| {
                let abs = bits & 0x7fff_ffff;
                abs <= 0x2f00_0000 || (abs > 0x5f00_0000 && abs < 0x7f80_0000)
            });
            let mut m = Space::new();
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x2000_0000 + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            let result = crate::native_matrix::from_euler_memory(
                &mut m,
                row["out"].as_u64().unwrap() as u32,
                row["source"].as_u64().unwrap() as u32,
                0x7fef0000,
            );
            let expected = if bounded || scalar {
                result.unwrap_or_else(|e| panic!("case{index}: {e}"));
                parity += 1;
                assert_eq!(row["trig"].as_array().unwrap().len(), 6);
                for observation in row["trig"].as_array().unwrap() {
                    assert_eq!(observation["fpcw"], 127);
                    let bits = observation["input"].as_u64().unwrap() as u32;
                    let cosine = observation["op"] == "cos";
                    let original = raw_image(observation["raw"].as_str().unwrap());
                    if bounded {
                        let provider = oracle_provider_points(bits).unwrap();
                        let enclosure = trig(bits).unwrap();
                        let (provider, enclosure) = if cosine {
                            (provider.1, enclosure.1)
                        } else {
                            (provider.0, enclosure.0)
                        };
                        assert_eq!(
                            provider.to_bytes(),
                            original.to_bytes(),
                            "case{index}: exact declared host providerpoint"
                        );
                        assert!(
                            enclosure.contains(original),
                            "case{index}: retained raw originalpoint outside admitted hull"
                        );
                    } else {
                        let expected = if cosine && bits & 0x7fff_ffff <= 0x2f00_0000 {
                            X87::from_i32(1)
                        } else {
                            X87::from_f32(f32::from_bits(bits))
                        };
                        assert_eq!(
                            expected.to_bytes(),
                            original.to_bytes(),
                            "case{index}: exact scalar-domainpoint"
                        );
                    }
                    raw_count += 1;
                }
                &row["after"]
            } else {
                assert!(result.unwrap_err().contains("NativeMatrixBoundary"));
                unsupported += 1;
                &row["before"]
            };
            for (i, word) in expected.as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x2000_0000 + i as u32 * 4),
                    word.as_u64().unwrap() as u32,
                    "case{index}: complete caller heap word{i}"
                );
            }
        }
        assert_eq!((parity, unsupported, raw_count), (1483, 32, 8898));
    }

    #[test]
    fn ambiguous_and_out_of_domain_stores_never_publish_partial_writes() {
        let mut m = Space::new();
        m.w32(0x2000_0010, 0x5a01_0000);
        let before = m.r32(0x2000_0010);
        let mut e = Evaluation {
            memory: &m,
            writes: vec![],
            stack: vec![],
        };
        e.push(Interval::bits(0x3f80_0000));
        e.store(0x2000_0010, true).unwrap();
        e.push(Interval {
            lo: X87::from_f32(1.0),
            hi: X87::from_f32(f32::from_bits(0x3f80_0001)),
        });
        assert!(e
            .store(0x2000_0014, true)
            .unwrap_err()
            .contains("inconclusive certified m32 store"));
        assert_eq!(m.r32(0x2000_0010), before);
        m.w32(0x2000_0060, 0x4100_0001);
        assert!(certify(&mut m, 0x2000_0010, 0x2000_0060, 0x7fef0000)
            .unwrap_err()
            .contains("certified standalone-model domain exceeded"));
        assert_eq!(m.r32(0x2000_0010), before);
    }
    #[test]
    fn reduced_angle_proof_matches_every_original_caller_heap_word() {
        let required_fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/checkpoints/20261021-campaign/car-command/euler-specials-v2.json");
        if crate::oracle_fixture::file(&required_fixture).is_none() {
            return;
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: serde_json::Value =
            serde_json::from_slice(
                &std::fs::read(root.join(
                    "re/out/checkpoints/20261021-campaign/car-command/euler-specials-v2.json",
                ))
                .unwrap(),
            )
            .unwrap();
        assert_eq!(data["rows"].as_array().unwrap().len(), 160);
        let mut accepted = 0;
        for (index, row) in data["rows"].as_array().unwrap().iter().enumerate() {
            let mut m = Space::new();
            for (i, v) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            let eligible = row["angles"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v.as_u64().unwrap() as u32 & 0x7fffffff <= 0x41000000);
            let result = certify(
                &mut m,
                row["out"].as_u64().unwrap() as u32,
                row["source"].as_u64().unwrap() as u32,
                0x7fef0000,
            );
            let expected = if eligible {
                result.unwrap_or_else(|e| panic!("case{index}: {e}"));
                accepted += 1;
                &row["after"]
            } else {
                assert!(result.is_err());
                &row["before"]
            };
            for (i, v) in expected.as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "case{index} full caller heap word{i}"
                );
            }
        }
        assert_eq!(accepted, 116);
    }

    #[test]
    fn reduced_angle_cube_certifies_original_outputs_or_stays_explicit() {
        let required_fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/certified-euler-original.json");
        if crate::oracle_fixture::file(&required_fixture).is_none() {
            return;
        }

        use sha2::{Digest, Sha256};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/certified-euler-original.json"))
                .expect("run certified_euler_probe.py"),
        )
        .unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(data["entry"], 0x54ecd0);
        assert_eq!(data["fpu_cw"], 0x7f);
        for (field, file) in [
            ("exe_sha256", "reference/raw/StreetLegal_Redline.bak.exe"),
            ("producer_sha256", "re/oracle/certified_euler_probe.py"),
            ("oracle_sha256", "re/oracle/oracle.py"),
        ] {
            assert_eq!(
                data[field],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(file)).unwrap())
                )
            );
        }
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 330);
        let (mut certified, mut inconclusive) = (0, 0);
        for (index, row) in rows.iter().enumerate() {
            let mut m = Space::new();
            for (i, v) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            let result = crate::native_matrix::from_euler_memory(
                &mut m,
                row["out"].as_u64().unwrap() as u32,
                row["source"].as_u64().unwrap() as u32,
                0x7fef0000,
            );
            let expected = match result {
                Ok(()) => {
                    certified += 1;
                    &row["after"]
                }
                Err(error) => {
                    assert!(
                        error.contains("inconclusive certified m32 store"),
                        "case{index} {error}"
                    );
                    inconclusive += 1;
                    &row["before"]
                }
            };
            for (i, v) in expected.as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "case{index} every caller heap word{i}"
                );
            }
        }
        eprintln!("reduced-angle certificate: {certified} exact caller heaps; {inconclusive} explicit inconclusive cases");
        assert_eq!(
            (certified, inconclusive),
            (330, 0),
            "unsupported observations do not pass the declared positive corpus"
        );
    }
}
