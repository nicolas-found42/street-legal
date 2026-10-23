//! The exe's "log and continue" paths, as recorded in `Alu::diagnostics`.
//!
//! Expected entries come from reading the helpers' pseudocode
//! (`re/out/all_pseudo.c`, log strings and the conditions guarding them),
//! one case per log site. Values for the non-log paths are covered by the
//! oracle vectors (`vm_oracle.rs`); the Unicorn sinks for these logs are
//! stubbed, so this file is static-evidence only.

use slr_vm::ops::{Alu, Diagnostic as D, LogKind as K, OpError};
use slr_vm::values::{TypeTag, Value};

fn log(helper: u32, kind: K, tag: u8) -> D {
    D::Logged { helper, kind, tag }
}

fn fl(v: f32) -> Value {
    Value::float(v)
}

fn other() -> Value {
    Value {
        tag: TypeTag::Other(b'X'),
        payload: 0,
    }
}

fn arr(p: u32) -> Value {
    Value {
        tag: TypeTag::Array("["),
        payload: p,
    }
}

fn run(f: impl FnOnce(&mut Alu)) -> Vec<D> {
    let mut a = Alu::default();
    f(&mut a);
    a.diagnostics
}

#[test]
fn integer_division_by_zero_logs_division_by_zero_then_traps() {
    let mut a = Alu::default();
    let r = a.div(&mut Value::int(5), &mut Value::int(0));
    assert_eq!(r, Err(OpError::IntegerTrap { helper: 0x41b970 }));
    assert_eq!(a.diagnostics, vec![log(0x41b970, K::DivisionByZero, b'I')]);
    let mut a = Alu::default();
    let r = a.rem(&mut Value::int(5), &mut Value::int(0));
    assert_eq!(r, Err(OpError::IntegerTrap { helper: 0x41bbf0 }));
    assert_eq!(a.diagnostics, vec![log(0x41bbf0, K::DivisionByZero, b'I')]);
    // INT_MIN / -1 traps without a log
    let mut a = Alu::default();
    assert!(a
        .div(&mut Value::int(i32::MIN), &mut Value::int(-1))
        .is_err());
    assert!(a.diagnostics.is_empty());
}

#[test]
fn float_division_logs_for_zero_and_nan_divisors_but_still_divides() {
    for bad in [0.0f32, -0.0, f32::NAN] {
        let mut a = Alu::default();
        let mut l = fl(1.0);
        a.div(&mut l, &mut fl(bad)).unwrap();
        assert_eq!(
            a.diagnostics,
            vec![log(0x41b970, K::DivisionByZero, b'F')],
            "divisor {bad}"
        );
    }
    let d = run(|a| a.div(&mut fl(1.0), &mut fl(2.0)).unwrap());
    assert!(d.is_empty());
}

#[test]
fn non_integral_operands_are_logged_by_shifts_bitwise_and_modulo() {
    let d = run(|a| a.shl(&mut fl(1.0), &mut fl(2.0)));
    assert_eq!(d, vec![log(0x41b0e0, K::NonIntegralOperands, b'F')]);
    let d = run(|a| a.shr(&mut fl(1.0), &mut fl(2.0)));
    assert_eq!(d, vec![log(0x41aff0, K::NonIntegralOperands, b'F')]);
    let d = run(|a| a.bit_or(&mut fl(1.0), &mut fl(2.0)));
    assert_eq!(d, vec![log(0x41c290, K::NonIntegralOperands, b'F')]);
    let d = run(|a| a.bit_xor(&mut fl(1.0), &mut fl(2.0)));
    assert_eq!(d, vec![log(0x41c380, K::NonIntegralOperands, b'F')]);
    let d = run(|a| a.bit_and(&mut fl(1.0), &mut fl(2.0)));
    assert_eq!(d, vec![log(0x41c1a0, K::NonIntegralOperands, b'F')]);
    let d = run(|a| {
        a.rem(&mut fl(1.0), &mut fl(2.0)).unwrap();
    });
    assert_eq!(d, vec![log(0x41bbf0, K::NonIntegralOperands, b'F')]);
    // logical operators log when the converted left cell is not an int
    let d = run(|a| a.logical_or(&mut other(), &mut Value::int(1)));
    assert_eq!(d, vec![log(0x41bfb0, K::NonIntegralOperands, b'X')]);
    let d = run(|a| a.logical_and(&mut other(), &mut Value::int(1)));
    assert_eq!(d, vec![log(0x41bea0, K::NonIntegralOperands, b'X')]);
}

#[test]
fn unary_helpers_log_their_own_unsupported_operand() {
    let r = Value::reference(5);
    let d = run(|a| a.bit_not(&mut r.clone()));
    assert_eq!(d, vec![log(0x41c0c0, K::NonIntegralOperand, b'L')]);
    let d = run(|a| a.logical_not(&mut other()));
    assert_eq!(d, vec![log(0x41bdb0, K::NonIntegralOperand, b'X')]);
    let d = run(|a| a.negate(&mut r.clone()));
    assert_eq!(d, vec![log(0x41b1d0, K::UnsupportedType, b'L')]);
    let d = run(|a| a.increment(&mut r.clone()));
    assert_eq!(d, vec![log(0x41b2d0, K::UnsupportedType, b'L')]);
    let d = run(|a| a.decrement(&mut r.clone()));
    assert_eq!(d, vec![log(0x41b3d0, K::UnsupportedType, b'L')]);
}

#[test]
fn arithmetic_helpers_log_unsupported_types() {
    let r = Value::reference(5);
    let d = run(|a| a.mul(&mut r.clone(), &mut r.clone()).unwrap());
    assert_eq!(d, vec![log(0x41b860, K::UnsupportedType, b'L')]);
    // `+` on references is the class path: "unsupported class type"
    let d = run(|a| a.add(&mut r.clone(), &mut r.clone()).unwrap());
    assert_eq!(d, vec![log(0x41b4d0, K::UnsupportedClassType, b'L')]);
    let d = run(|a| a.add(&mut other(), &mut other()).unwrap());
    assert_eq!(d, vec![log(0x41b4d0, K::UnsupportedType, b'X')]);
    // `-` negates the right operand first: that helper logs too
    let d = run(|a| a.sub(&mut Value::int(1), &mut r.clone()).unwrap());
    assert!(d.contains(&log(0x41b760, K::UnsupportedType, b'L')));
    let d = run(|a| a.div(&mut r.clone(), &mut r.clone()).unwrap());
    assert_eq!(d, vec![log(0x41b970, K::UnsupportedType, b'L')]);
}

#[test]
fn comparison_helpers_log_unsupported_eq_and_gt() {
    let r = Value::reference(5);
    let d = run(|a| a.eq(&mut other(), &mut other()));
    assert_eq!(d, vec![log(0x41c5c0, K::UnsupportedEq, b'X')]);
    let d = run(|a| a.ne(&mut other(), &mut other()));
    assert_eq!(d, vec![log(0x41c6c0, K::UnsupportedEq, b'X')]);
    // references compare by payload (no log) ...
    let d = run(|a| a.eq(&mut r.clone(), &mut r.clone()));
    assert!(d.is_empty());
    let d = run(|a| a.ge(&mut r.clone(), &mut r.clone()));
    assert!(d.is_empty(), "equal references: >= is decided by ==");
    // ... but ordering them logs "unsupported type (>)"
    let d = run(|a| a.gt(&mut r.clone(), &mut r.clone()));
    assert_eq!(d, vec![log(0x41c790, K::UnsupportedGt, b'L')]);
    let d = run(|a| a.le(&mut r.clone(), &mut r.clone()));
    assert_eq!(d, vec![log(0x41cb60, K::UnsupportedGt, b'L')]);
    let d = run(|a| a.ge(&mut r.clone(), &mut Value::reference(6)));
    assert_eq!(d, vec![log(0x41c860, K::UnsupportedGt, b'L')]);
    let d = run(|a| a.lt(&mut r.clone(), &mut Value::reference(6)));
    assert_eq!(d, vec![log(0x41c9e0, K::UnsupportedGt, b'L')]);
    // an unknown type logs both messages in the helpers that test == first
    let d = run(|a| a.lt(&mut other(), &mut other()));
    assert_eq!(
        d,
        vec![
            log(0x41c9e0, K::UnsupportedEq, b'X'),
            log(0x41c9e0, K::UnsupportedGt, b'X')
        ]
    );
}

#[test]
fn numeric_promotion_logs_cannot_promote_for_f_i_and_l_destinations() {
    let cp = |dst: u8, src: u8| D::CannotPromote(slr_vm::values::PromoteError { dst, src });
    let r = Value::reference(5);
    // F dst with a reference source
    let d = run(|a| a.bit_or(&mut fl(1.0), &mut r.clone()));
    assert!(d.contains(&cp(b'F', b'L')), "{d:?}");
    // I dst with a reference source
    let d = run(|a| a.bit_or(&mut Value::int(1), &mut r.clone()));
    assert!(d.contains(&cp(b'I', b'L')), "{d:?}");
    // L dst with a non-L source (an array counts as non-L here)
    let d = run(|a| a.eq(&mut r.clone(), &mut arr(8)));
    assert!(d.contains(&cp(b'L', b'[')), "{d:?}");
    // an array destination is silent, like the exe
    let d = run(|a| a.eq(&mut arr(8), &mut Value::int(1)));
    assert!(!d.iter().any(|x| matches!(x, D::CannotPromote(_))), "{d:?}");
}

#[test]
fn assignment_dispatcher_logs_its_own_errors() {
    for kind in [0x29u32, 0x2a, 0x2b, 0x99] {
        let d = run(|a| {
            a.assign(&mut Value::int(1), false, &mut Value::int(2), kind)
                .unwrap()
        });
        assert_eq!(
            d,
            vec![log(0x41acb0, K::UnsupportedAssignmentType, b'I')],
            "kind {kind:#x}"
        );
    }
    for kind in [0x2cu32, 0x2d, 0x2e] {
        let d = run(|a| {
            a.assign(&mut fl(1.0), false, &mut fl(2.0), kind).unwrap();
        });
        assert_eq!(
            d,
            vec![log(0x41acb0, K::NonIntegralOperands, b'F')],
            "kind {kind:#x}"
        );
    }
}

#[test]
fn storing_into_an_unknown_type_logs_unsupported_assign() {
    let d = run(|a| a.store(&mut other(), &mut Value::int(1)));
    assert!(
        d.contains(&log(0x41ab60, K::UnsupportedAssign, b'X')),
        "{d:?}"
    );
}

/// Historical loader-less behavior (kept for `Alu::store` without a loader):
/// with no class model the port logs the failed assignment conversion and
/// copies the raw payload word. `Alu::store_with` + a loader converts
/// instead (see `loader_converts_int_to_string_on_store`); the interpreter
/// always passes its loader, so this path is unreachable on live scripts.
#[test]
fn loaderless_int_or_float_into_a_reference_variable_copies_the_raw_word() {
    for (src, word) in [(Value::int(7), 7u32), (fl(2.5), 2.5f32.to_bits())] {
        let mut a = Alu::default();
        let mut dst = Value::reference(0);
        let mut s = src;
        a.store(&mut dst, &mut s);
        assert_eq!(dst, Value::reference(word), "raw word copied");
        assert!(
            a.diagnostics
                .iter()
                .any(|d| matches!(d, D::AssignConversion(_))),
            "the failed conversion is logged: {:?}",
            a.diagnostics
        );
        assert_eq!(a.ref_stores, vec![src], "still seen as a reference store");
    }
}

#[test]
fn loader_converts_int_to_string_on_store() {
    use slr_vm::values::{LoaderConvert, TypeTag, Value};
    struct Fake {
        next: u32,
    }
    impl LoaderConvert for Fake {
        fn load_class(&mut self, d: &str) -> Option<String> {
            matches!(d, "java.lang.String" | "Ljava.lang.String;")
                .then(|| "java.lang.String".to_owned())
        }
        fn is_assignable(&mut self, from: &str, to: &str) -> bool {
            from == "java.lang.String" && matches!(to, "java.lang.String" | "Ljava.lang.String;")
        }
        fn make_string(&mut self, _v: Value) -> u32 {
            let w = self.next;
            self.next += 1;
            w
        }
        fn target_name(&self, _d: &str) -> &'static str {
            TypeTag::STRING
        }
    }
    let mut a = Alu::default();
    let mut fake = Fake { next: 0x3000 };
    // A variable slot declared `Ljava.lang.String;` (the interpreter types
    // slots with full names; `Value::reference` defaults to bare `L;`).
    let mut dst = Value::reference_named(0, "Ljava.lang.String;");
    let mut src = Value::int(7);
    a.store_with(&mut dst, &mut src, Some(&mut fake));
    assert_eq!(dst.tag, TypeTag::Ref(TypeTag::STRING));
    assert_eq!(dst.payload, 0x3000, "interned String object word");
    assert!(
        a.diagnostics.is_empty(),
        "no log on success: {:?}",
        a.diagnostics
    );
}
