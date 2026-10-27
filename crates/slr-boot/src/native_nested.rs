//! Nested VM natives inside an engine-issued script callback.
//!
//! The original reaches such a native through 425a90 -> 4255e0 -> 41fbc0 ->
//! the native CALL: 0x530 bytes of frames (the Osd animate lastKey capture)
//! built from raw JVM thread, frame and CallInfo state the port never
//! constructs. A nested native is therefore admitted only when
//!
//! 1. the script was entered by the engine from a validated wrapper CALL
//!    (return PC and arguments on the native stack) and no native frame has
//!    been entered or called since, and
//! 2. its production implementation is audited not to read or write native
//!    memory, so its result cannot depend on those frames.
//!
//! Not modelled: the original native's own stack scratch writes below its
//! entry ESP, and the intermediate wrapper/interpreter frames themselves.
//! Every other nested native stays rejected by the ABI guard.
use crate::native_stack as stack;
use slr_physics::space::Space;
use std::cell::RefCell;

/// Production implementations audited to touch no native memory.
///
/// - `Input.lastKey()I`: `slr_io::input::bridge` over the Rust `Input` state,
///   or `live_input`'s key queue in front of it in the window build.
/// - `System.currentTime()F`: `slr_lang` `current_time` through
///   `LangHost::read_clock`, i.e. `sched::clock_ms` (replay tape or the
///   simulated clock).
/// - `GameRef.getScriptInstance()Ljava.lang.Object;`: the default
///   `slr_resource::node::bridge` over the Rust resource host (container type
///   and instance script word); its resource events are only a log.
/// - `System.arraycopy(Object;IObject;II)V`: `main.rs` `array_copy` over VM
///   array cells only.
/// - `RenderRef.setMatrix(Vector3;Ypr)V`: `capture_only` headless (no effect),
///   and `presentation`'s Rust render state in the window build.
/// - `Vector3.length()F`, `String.length()I`, `String.append(I)`: `slr_lang`
///   over VM values only.
/// - `ResourceRef.newNative()V`: the default `slr_resource` bridge.
/// - `ResourceRef.set(I)V`: `main.rs` `resource_set`, package metadata in the
///   Rust resource loader plus the `slr_resource` bridge.
/// - `ResourceRef.cache()V`: `materialise::load_like`, Rust resource state and a
///   stock file stat.
/// - `ResourceRef.destroy()V`: `resource_identity` queues a deferred release;
///   the destruction itself runs later in the frame's resource cleanup.
/// - `Text.update()V`, `Text.changeColor(I)V` (`slr_render_natives`) and
///   `SfxRef.nplay(...)I` (`capture_only` / `slr_sound`): crates that depend
///   only on `slr-vm` and cannot reach native memory.
const AUDITED: [(&str, &str, &str); 15] = [
    ("java.io.Input", "lastKey", "()I"),
    ("java.lang.System", "currentTime", "()F"),
    (
        "java.util.resource.GameRef",
        "getScriptInstance",
        "()Ljava.lang.Object;",
    ),
    (
        "java.lang.System",
        "arraycopy",
        "(Ljava.lang.Object;ILjava.lang.Object;II)V",
    ),
    (
        "java.util.resource.RenderRef",
        "setMatrix",
        "(Ljava.lang.Vector3;Ljava.lang.Ypr;)V",
    ),
    ("java.lang.Vector3", "length", "()F"),
    ("java.lang.String", "length", "()I"),
    ("java.lang.String", "append", "(I)Ljava.lang.String;"),
    ("java.util.resource.ResourceRef", "newNative", "()V"),
    ("java.util.resource.ResourceRef", "set", "(I)V"),
    ("java.util.resource.ResourceRef", "cache", "()V"),
    ("java.util.resource.ResourceRef", "destroy", "()V"),
    ("java.render.Text", "update", "()V"),
    ("java.render.Text", "changeColor", "(I)V"),
    (
        "java.util.resource.SfxRef",
        "nplay",
        "(Ljava.lang.Vector3;FFFII)I",
    ),
];

/// Original CALL sites of the 425a90 wrapper: the return PC and the words
/// the caller pushed after the envelope, `None` where the word varies.
const SITES: [(u32, &[Option<u32>]); 3] = [
    // 427111, the animate list pass: "animate", 0, 0x4f.
    (0x427111, &[Some(0x60cae4), Some(0), Some(0x4f)]),
    // 426da3, the control list pass: "control", 0, 3, dt as f64, 0x4f.
    (
        0x426da3,
        &[Some(0x60cf38), Some(0), Some(3), None, None, Some(0x4f)],
    ),
    // 447c41, car add-part 447b30: "addpart", 0, 4, GameRef object, 0x4f.
    (
        0x447c41,
        &[Some(0x610000), Some(0), Some(4), None, Some(0x4f)],
    ),
];

/// A validated wrapper CALL: the native stack as the engine entered it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entered {
    esp: u32,
    depth: u32,
}

thread_local! {
    static ENTERED: RefCell<Vec<Option<Entered>>> = const { RefCell::new(Vec::new()) };
}

/// Check the entered wrapper frame against a known original CALL site: its
/// return PC, the envelope and every fixed argument word, with no pending CALL.
pub fn validate(m: &Space, envelope: u32) -> Option<Entered> {
    if !stack::active(m) || stack::has_pending_call(m) {
        return None;
    }
    let esp = stack::sp(m);
    let (_, words) = SITES.iter().find(|(pc, _)| m.r32(esp) == *pc)?;
    let fixed = words
        .iter()
        .enumerate()
        .all(|(index, word)| word.is_none_or(|word| m.r32(esp + 8 + index as u32 * 4) == word));
    (m.r32(esp + 4) == envelope && fixed).then(|| Entered {
        esp,
        depth: stack::depth(m),
    })
}

/// Run `f` as the innermost engine script callback. `None` marks a callback
/// whose site is not validated; nested natives inside it stay rejected.
pub fn within<R>(entered: Option<Entered>, f: impl FnOnce() -> R) -> R {
    struct Pop;
    impl Drop for Pop {
        fn drop(&mut self) {
            ENTERED.with(|entered| entered.borrow_mut().pop());
        }
    }
    ENTERED.with(|stack| stack.borrow_mut().push(entered));
    let _pop = Pop;
    f()
}

/// Whether a native entered while the native stack is active may run.
pub fn admits(m: &Space, class: &str, name: &str, signature: &str) -> bool {
    let Some(Some(entered)) = ENTERED.with(|stack| stack.borrow().last().copied()) else {
        return false;
    };
    stack::sp(m) == entered.esp
        && stack::depth(m) == entered.depth
        && !stack::has_pending_call(m)
        && AUDITED.contains(&(class, name, signature))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENVELOPE: u32 = 0x2000_7818;

    fn animate_callback<R>(m: &mut Space, f: impl FnOnce(&mut Space) -> R) -> R {
        stack::function(m, &[], None, |m| {
            stack::call(m, 0x427111, &[ENVELOPE, 0x60cae4, 0, 0x4f], None, |m| {
                stack::function_cdecl(m, &[ENVELOPE, 0x60cae4, 0, 0x4f], f)
            })
        })
    }

    #[test]
    fn audited_native_is_admitted_only_in_a_validated_untouched_callback() {
        let mut m = Space::new();
        animate_callback(&mut m, |m| {
            let entered = validate(m, ENVELOPE).expect("validated wrapper CALL");
            assert!(!admits(m, "java.io.Input", "lastKey", "()I"));
            within(Some(entered), || {
                assert!(admits(m, "java.io.Input", "lastKey", "()I"));
                assert!(admits(m, "java.lang.System", "currentTime", "()F"));
                // Unaudited natives keep the guard.
                assert!(!admits(m, "java.io.Input", "checkHotkeys", "()V"));
                assert!(!admits(m, "java.io.Input", "lastKey", "()F"));
                // A pending CALL or a native frame entered since the wrapper
                // invalidates it.
                stack::call(m, 0x401000, &[1], None, |m| {
                    assert!(!admits(m, "java.io.Input", "lastKey", "()I"));
                    stack::function(m, &[1], None, |m| {
                        assert!(!admits(m, "java.io.Input", "lastKey", "()I"));
                    });
                });
                assert!(admits(m, "java.io.Input", "lastKey", "()I"));
                // An unvalidated inner callback rejects nested natives.
                within(None, || {
                    assert!(!admits(m, "java.io.Input", "lastKey", "()I"));
                });
            });
            assert!(!admits(m, "java.io.Input", "lastKey", "()I"));
        });
    }

    #[test]
    fn wrapper_validation_requires_the_actual_return_pc_and_arguments() {
        let mut m = Space::new();
        animate_callback(&mut m, |m| {
            assert!(validate(m, ENVELOPE + 4).is_none());
            // The animate site's fixed words must all match.
            m.w32(stack::sp(m) + 12, 1);
            assert!(validate(m, ENVELOPE).is_none());
        });
        stack::function(&mut m, &[], None, |m| {
            stack::call(m, 0x427112, &[ENVELOPE, 0x60cae4, 0, 0x4f], None, |m| {
                stack::function_cdecl(m, &[ENVELOPE, 0x60cae4, 0, 0x4f], |m| {
                    assert!(validate(m, ENVELOPE).is_none());
                })
            })
        });
        assert!(validate(&m, ENVELOPE).is_none());
        stack::function(&mut m, &[], None, |m| {
            let object = 0x2000_1000;
            stack::call(
                m,
                0x447c41,
                &[ENVELOPE, 0x610000, 0, 4, object, 0x4f],
                None,
                |m| {
                    stack::function_cdecl(m, &[], |m| {
                        let entered = validate(m, ENVELOPE).expect("car add-part CALL");
                        within(Some(entered), || {
                            assert!(admits(
                                m,
                                "java.util.resource.GameRef",
                                "getScriptInstance",
                                "()Ljava.lang.Object;"
                            ));
                        });
                    })
                },
            )
        });
    }
}
