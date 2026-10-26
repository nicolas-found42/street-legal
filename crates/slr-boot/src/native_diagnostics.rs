//! Observed state at the native/VM boundary for run bundles. Observers only
//! read memory: they never enter natives, read the clock or change scheduling.
use crate::native_stack::{self as stack, Reg};
use serde_json::{json, Value};
use slr_physics::space::Space;
use slr_vm::diagnostics::{self, EventKind};
use std::cell::RefCell;

/// Native stack words captured above ESP at a rejected nested native.
const WINDOW_WORDS: u32 = 64;

/// The engine site that entered a script from native code.
#[derive(Clone)]
pub struct Scope {
    pub kind: &'static str,
    pub envelope: u32,
    pub script: u32,
    pub method: String,
    /// Whether `native_nested` validated the wrapper CALL that entered it.
    pub validated: bool,
}
impl Scope {
    fn fields(&self) -> Vec<(String, String)> {
        vec![
            ("kind".into(), self.kind.into()),
            ("envelope".into(), hex(self.envelope)),
            ("script".into(), hex(self.script)),
            ("method".into(), self.method.clone()),
            ("validated".into(), self.validated.to_string()),
        ]
    }
    fn json(&self) -> Value {
        json!({"kind":self.kind,"envelope":hex(self.envelope),"script":hex(self.script),"method":self.method,"validated":self.validated})
    }
}

thread_local! {
    static SCOPES: RefCell<Vec<Scope>> = const { RefCell::new(Vec::new()) };
    static LAST_BOUNDARY: RefCell<Option<Value>> = const { RefCell::new(None) };
}

fn hex(word: u32) -> String {
    format!("{word:#010x}")
}

fn scopes() -> Value {
    SCOPES.with(|scopes| Value::Array(scopes.borrow().iter().map(Scope::json).collect()))
}

/// Run a script entered from native code, bracketing it with enter/return
/// events. Nested natives inside `f` see this scope in their boundary event.
pub fn script_callback<R>(
    scope: Scope,
    f: impl FnOnce() -> Result<R, String>,
) -> Result<R, String> {
    diagnostics::emit(EventKind::External {
        category: "script-callback-enter".into(),
        fields: scope.fields(),
    });
    SCOPES.with(|scopes| scopes.borrow_mut().push(scope.clone()));
    let result = f();
    SCOPES.with(|scopes| scopes.borrow_mut().pop());
    let mut fields = scope.fields();
    fields.push((
        "error".into(),
        result
            .as_ref()
            .err()
            .cloned()
            .unwrap_or_else(|| "none".into()),
    ));
    diagnostics::emit(EventKind::External {
        category: "script-callback-return".into(),
        fields,
    });
    result
}

/// Registers, depth, pending CALL and the raw words above ESP. Words are not
/// interpreted as return addresses; the frame chain is not reconstructed.
pub fn stack_state(m: &Space) -> Value {
    let active = stack::active(m);
    let esp = stack::sp(m);
    let window: Vec<Value> = if active {
        (0..WINDOW_WORDS)
            .map(|i| esp.wrapping_add(i * 4))
            .take_while(|address| *address < stack::top())
            .map(|address| json!([hex(address), hex(m.r32(address))]))
            .collect()
    } else {
        Vec::new()
    };
    let registers: serde_json::Map<String, Value> = [
        ("eax", Reg::Eax),
        ("ebx", Reg::Ebx),
        ("ecx", Reg::Ecx),
        ("edx", Reg::Edx),
        ("esi", Reg::Esi),
        ("edi", Reg::Edi),
        ("ebp", Reg::Ebp),
    ]
    .into_iter()
    .map(|(name, r)| (name.to_string(), Value::String(hex(stack::reg(m, r)))))
    .collect();
    json!({"active":active,"depth":stack::depth(m),"pending_call":stack::has_pending_call(m),
        "esp":hex(esp),"used_bytes":if active {stack::top().wrapping_sub(esp)} else {0},
        "registers":registers,"window":window,"window_limit":WINDOW_WORDS})
}

/// Record a rejected native entry with the engine scopes and native stack.
pub fn boundary(class: &str, name: &str, signature: &str, reason: &str) {
    let stack = crate::native_body::inspect_world_memory(stack_state)
        .unwrap_or_else(|| json!({"unavailable":"shared native memory is leased"}));
    let value = json!({"native":format!("{class}.{name}{signature}"),"reason":reason,
        "scopes":scopes(),"stack":stack});
    let sequence = diagnostics::emit(EventKind::External {
        category: "native-abi-boundary".into(),
        fields: vec![
            ("native".into(), format!("{class}.{name}{signature}")),
            ("reason".into(), reason.into()),
            ("scopes".into(), value["scopes"].to_string()),
            ("stack".into(), value["stack"].to_string()),
        ],
    });
    let mut value = value;
    value["sequence"] = json!(sequence);
    LAST_BOUNDARY.with(|last| *last.borrow_mut() = Some(value));
}

/// Record a nested native admitted by `native_nested`.
pub fn admitted(class: &str, name: &str, signature: &str) {
    diagnostics::emit(EventKind::External {
        category: "native-nested-admitted".into(),
        fields: vec![
            ("native".into(), format!("{class}.{name}{signature}")),
            ("scopes".into(), scopes().to_string()),
        ],
    });
}

/// The boundary section of a state snapshot.
pub fn state() -> Value {
    json!({"scopes":scopes(),"last_boundary":LAST_BOUNDARY.with(|last| last.borrow().clone())})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_state_reports_entered_frame_words_without_changing_memory() {
        let mut m = Space::new();
        stack::function(&mut m, &[0x11, 0x22], None, |m| {
            let before = stack::sp(m);
            let state = stack_state(m);
            assert_eq!(stack::sp(m), before);
            assert_eq!(state["active"], json!(true));
            assert_eq!(state["depth"], json!(1));
            assert_eq!(state["window"][1], json!([hex(before + 4), hex(0x11)]));
            assert_eq!(state["window"][2], json!([hex(before + 8), hex(0x22)]));
        });
        let idle = stack_state(&m);
        assert_eq!(idle["active"], json!(false));
        assert_eq!(idle["window"], json!([]));
    }

    #[test]
    fn script_callback_scope_is_visible_only_while_the_script_runs() {
        let scope = Scope {
            kind: "animate",
            envelope: 0x1000,
            script: 0x1486,
            method: "animate".into(),
            validated: true,
        };
        let seen = script_callback(scope, || Ok::<_, String>(scopes())).unwrap();
        assert_eq!(seen[0]["script"], json!("0x00001486"));
        assert_eq!(scopes(), json!([]));
        let failed = script_callback(
            Scope {
                kind: "drive",
                envelope: 0,
                script: 0,
                method: "drive".into(),
                validated: false,
            },
            || Err::<(), _>("boom".to_string()),
        );
        assert_eq!(failed.unwrap_err(), "boom");
        assert_eq!(scopes(), json!([]));
    }
}
