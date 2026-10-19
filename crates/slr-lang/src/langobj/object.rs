//! `java.lang.Object` natives — M1-08 rows O1..O4.
//!
//! Four exe natives, each a six-instruction `CallInfo` wrapper over one
//! shared helper (`0x47c870-0x47c916`, read off the machine code and
//! re-run under Unicorn in this session):
//!
//! | exe | registry row | body |
//! |---|---|---|
//! | [`TO_STRING`] `0x0047c870` | `toString()Ljava.lang.String;` | class name + `"%s<%0x>"` + intern |
//! | [`DISABLE_GC`] `0x0047c8c0` | `disableGC()V` | tail-call `FUN_00408720(obj)` |
//! | [`ENABLE_GC`] `0x0047c8e0` | `enableGC()V` | tail-call `FUN_00408780(obj)` |
//! | [`HASH_CODE`] `0x0047c900` | `hashCode()I` | `eax` = `out[0]` (the receiver word) |
//!
//! Every one of them marshals through `FUN_0045d910` with a one-word out
//! array, so the only argument is the receiver/return slot at
//! `container[count-1]` ([`slr_vm::host::marshal_args`]).
//!
//! # `toString`
//!
//! ```text
//! 47c889  eax = out[0]                 ; receiver word
//! 47c88d  ecx = [eax+0xc]              ; class descriptor
//! 47c894  call FUN_00404ea0            ; thiscall: name = [[ecx+8]]
//! 47c89e  push 0x612d1c                ; "%s<%0x>"  (literal read from the exe)
//! 47c8a4  call FUN_00551220            ; sprintf(buf, "%s<%0x>", name, obj)
//! 47c8b3  call FUN_004174a0            ; intern(jvm, buf) -> String object
//! ```
//!
//! The format is the CRT's `%0x`, i.e. lowercase hex with **no** width and no
//! padding: `0` renders `0`, `0xabcdef` renders `abcdef` (probed through the
//! exe's own `sprintf` at `0x551220`). The object word is printed as the raw
//! 32-bit word, exactly as the exe does (the port's words are its arena
//! words, so the text differs from a live exe run; the *rule* is what is
//! ported and asserted).
//!
//! # `disableGC` / `enableGC`
//!
//! Both move the object between two intrusive lists owned by the class data
//! (`*(*(obj+0xc)+0xc)`), keyed off flag bit `4` in the object word at
//! `obj+0x18`:
//!
//! - `disableGC` (`FUN_00408720`): if bit 4 is clear, unlink
//!   `(+4 next, +8 prev)`, then insert **after** the head node
//!   `list = classdata+0x14`, `head = *(list+0x18)`:
//!   `obj.next = head.next`, `obj.prev = head`, `head.next = obj`,
//!   `*(list+0x18) = obj`; finally `flags |= 4`.
//! - `enableGC` (`FUN_00408780`): if bit 4 is set, unlink, then insert
//!   **before** the head node of `classdata+0x10`, `head = *(list+8)`:
//!   `obj.next = head`, `obj.prev = head.prev`, `head.prev = obj`,
//!   `*(list+8) = obj`; finally `flags &= ~4`.
//!
//! Two quirks are reproduced deliberately because they are observable
//! through the link words:
//!
//! - The head cell *is* the head node pointer and is rewritten to the new
//!   object on every insert, so after the first insert the previous object
//!   plays the head node's role (`head.next` is what the next insert reads).
//! - An unlink never repairs the opposite link of the outgoing neighbour,
//!   and it leaves the head cell pointing at the object that moved away
//!   (probe: after `enableGC` of the last disabled object the disabled head
//!   still named it).
//!
//! # Seam
//!
//! The exe keeps the object heap and the class data in the JVM context. The
//! port's object heap is [`VmHost::objects`] and its class data has no word
//! for the two list heads yet, so the lists live in [`GC_LISTS`], a
//! thread-local [`GcLists`] keyed by class name (the exe keys by class-data
//! pointer; the name is the port's identity for it). The natives themselves
//! are [`slr_vm::host::NativeFn`]-shaped — they need the object model, which
//! [`crate::LangHost`] does not carry — and install through
//! [`crate::NativeTable::set_vm`].

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(test)]
use slr_vm::host::Object;
use slr_vm::host::{marshal_args, CallInfo, VmHost};
#[cfg(test)]
use slr_vm::values::Value;

use crate::seam::{NativeBinding, NativeTable};

/// Exe address of `java.lang.Object.toString()Ljava.lang.String;`.
pub const TO_STRING: u32 = 0x0047_c870;
/// Exe address of `java.lang.Object.disableGC()V`.
pub const DISABLE_GC: u32 = 0x0047_c8c0;
/// Exe address of `java.lang.Object.enableGC()V`.
pub const ENABLE_GC: u32 = 0x0047_c8e0;
/// Exe address of `java.lang.Object.hashCode()I`.
pub const HASH_CODE: u32 = 0x0047_c900;

/// `java.lang.Object` in the registry spelling.
pub const OBJECT: &str = "java.lang.Object";

/// The registry rows these ports answer (`re/out/native_api.tsv` 151-154).
pub const OBJECT_NATIVE_BINDINGS: &[NativeBinding] = &[
    NativeBinding {
        class: OBJECT,
        name: "toString",
        sig: "()Ljava.lang.String;",
        impl_addr: TO_STRING,
    },
    NativeBinding {
        class: OBJECT,
        name: "disableGC",
        sig: "()V",
        impl_addr: DISABLE_GC,
    },
    NativeBinding {
        class: OBJECT,
        name: "enableGC",
        sig: "()V",
        impl_addr: ENABLE_GC,
    },
    NativeBinding {
        class: OBJECT,
        name: "hashCode",
        sig: "()I",
        impl_addr: HASH_CODE,
    },
];

// ---- the GC list model -----------------------------------------------

/// Flag bit 4 in the object word (`obj+0x18`): "GC disabled"
/// (`FUN_00408720`/`FUN_00408780`). The other bits belong to the object
/// model and are carried untouched.
pub const GC_DISABLED: u32 = 0x4;

/// One object's GC words: `+4` next, `+8` prev, `+0x18` flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GcNode {
    /// `obj+4`: next object in the list (0 = none).
    pub next: u32,
    /// `obj+8`: previous object in the list (0 = none).
    pub prev: u32,
    /// `obj+0x18`: flags word; [`GC_DISABLED`] is the only bit these two
    /// natives read or write.
    pub flags: u32,
}

/// One list head: the class-data cell (`+0x10` normal, `+0x14` disabled,
/// through its list object's `+8`/`+0x18`) plus the sentinel node whose word
/// the cell holds before the first insert.
///
/// The exe rewrites the cell to the inserted object, so `head` and
/// `sentinel` diverge after the first insert — both are kept because the
/// walker and the link values need them separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GcList {
    /// The class-data head cell: current first object, or [`GcList::sentinel`].
    pub head: u32,
    /// The initial head node word (its links start zeroed).
    pub sentinel: u32,
}

/// One class context's two lists plus the link words of every object that
/// has been through them (the exe stores them in the objects themselves).
#[derive(Clone, Debug)]
pub struct ClassGc {
    /// The GC-enabled list (`classdata+0x10`).
    pub normal: GcList,
    /// The GC-disabled list (`classdata+0x14`).
    pub disabled: GcList,
    nodes: HashMap<u32, GcNode>,
}

impl ClassGc {
    fn new(sentinel: u32) -> Self {
        Self {
            normal: GcList {
                head: sentinel,
                sentinel,
            },
            disabled: GcList {
                head: sentinel + 1,
                sentinel: sentinel + 1,
            },
            nodes: HashMap::new(),
        }
    }

    /// The object's GC words (a never-seen object is all zeroes, exactly the
    /// zero-initialized fields the exe's allocator hands out).
    #[must_use]
    pub fn node(&self, word: u32) -> GcNode {
        self.nodes.get(&word).copied().unwrap_or_default()
    }

    fn node_mut(&mut self, word: u32) -> &mut GcNode {
        self.nodes.entry(word).or_default()
    }

    /// `obj+4`/`obj+8`.
    #[must_use]
    pub fn links(&self, word: u32) -> (u32, u32) {
        let n = self.node(word);
        (n.next, n.prev)
    }

    /// `obj+0x18`.
    #[must_use]
    pub fn flags(&self, word: u32) -> u32 {
        self.node(word).flags
    }

    /// The unlink both natives share (`0x408726-0x408740`,
    /// `0x408786-0x4087a0`): repair both neighbours only when both links are
    /// set, then zero the object's own links. The head cell is deliberately
    /// left alone (see the module docs).
    fn unlink(&mut self, word: u32) {
        let n = self.node(word);
        if n.next != 0 && n.prev != 0 {
            self.node_mut(n.next).prev = n.prev;
            self.node_mut(n.prev).next = n.next;
        }
        let m = self.node_mut(word);
        m.next = 0;
        m.prev = 0;
    }
}

/// The per-class GC lists (`GcLists`), keyed by class name.
#[derive(Clone, Debug, Default)]
pub struct GcLists {
    classes: HashMap<String, ClassGc>,
    next_sentinel: u32,
}

impl GcLists {
    /// Words handed out to the sentinel nodes. They are never object words
    /// (the port's object words are `0x1000`-based, `0x2000_0000`-based or
    /// `0x3000_0000`-based), so a link value is unambiguous.
    const SENTINEL_BASE: u32 = 0x8000_0000;

    /// An empty set: no class has been touched, so no sentinel exists yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            classes: HashMap::new(),
            next_sentinel: Self::SENTINEL_BASE,
        }
    }

    /// The lists of `class`, or `None` before the first native touched it
    /// (the exe's class data exists from class load; the port creates it
    /// lazily because no ported path observes an untouched class).
    #[must_use]
    pub fn class(&self, class: &str) -> Option<&ClassGc> {
        self.classes.get(class)
    }

    /// Number of class contexts that have been touched.
    #[must_use]
    pub fn classes(&self) -> usize {
        self.classes.len()
    }

    fn entry(&mut self, class: &str) -> &mut ClassGc {
        if !self.classes.contains_key(class) {
            let sentinel = self.next_sentinel;
            self.next_sentinel += 2;
            self.classes
                .insert(class.to_owned(), ClassGc::new(sentinel));
        }
        self.classes
            .get_mut(class)
            .expect("class inserted immediately above")
    }

    /// `FUN_00408720(obj)`: move `word` into the disabled list of `class`.
    ///
    /// Returns `false` when the object was already disabled (bit 4 set): the
    /// exe's early-out, which touches nothing.
    ///
    /// exe: FUN_00408720
    pub fn disable(&mut self, class: &str, word: u32) -> bool {
        // The exe dereferences the object; a null receiver is an exe fault,
        // so the port does nothing rather than inventing list state.
        if word == 0 {
            return false;
        }
        let g = self.entry(class);
        if g.node(word).flags & GC_DISABLED != 0 {
            return false;
        }
        g.unlink(word);
        let head = g.disabled.head;
        let next = g.node(head).next;
        let n = g.node_mut(word);
        n.next = next;
        n.prev = head;
        g.node_mut(head).next = word;
        g.disabled.head = word;
        g.node_mut(word).flags |= GC_DISABLED;
        true
    }

    /// `FUN_00408780(obj)`: move `word` into the normal list of `class`.
    ///
    /// Returns `false` when the object was not disabled (bit 4 clear): the
    /// exe's early-out.
    ///
    /// exe: FUN_00408780
    pub fn enable(&mut self, class: &str, word: u32) -> bool {
        if word == 0 {
            return false;
        }
        let g = self.entry(class);
        if g.node(word).flags & GC_DISABLED == 0 {
            return false;
        }
        g.unlink(word);
        let head = g.normal.head;
        let prev = g.node(head).prev;
        let n = g.node_mut(word);
        n.next = head;
        n.prev = prev;
        g.node_mut(head).prev = word;
        g.normal.head = word;
        g.node_mut(word).flags &= !GC_DISABLED;
        true
    }
}

thread_local! {
    /// The exe's class-data list heads, per class (see the module docs).
    static GC_LISTS: RefCell<GcLists> = RefCell::new(GcLists::new());
}

/// Run `f` over this thread's [`GcLists`].
pub fn with_gc_lists<R>(f: impl FnOnce(&mut GcLists) -> R) -> R {
    GC_LISTS.with(|l| f(&mut l.borrow_mut()))
}

/// Drop every list this thread built (fresh-runtime reset, same shape as
/// [`crate::seam::reset`]).
pub fn reset() {
    GC_LISTS.with(|l| *l.borrow_mut() = GcLists::new());
}

// ---- the natives ------------------------------------------------------

/// The receiver/return slot the natives read: `out[0]` after
/// `FUN_0045d910`, which is `container[count-1]`.
///
/// A marshalling failure leaves the exe's out word at whatever the caller's
/// stack held; the port reads 0 there and returns the null result, which is
/// the only value the port can state truthfully.
fn receiver(info: &CallInfo<'_>) -> u32 {
    let mut out = [0u32; 1];
    match marshal_args(info, &mut out) {
        Ok(_) => out[0],
        Err(_) => 0,
    }
}

/// Decode a port object word to its index in [`VmHost::objects`].
///
/// The port's three allocators hand out `base + index` words:
/// `VmHost::alloc_object` (`0x1000`), `VmHost::make_string`
/// (`0x2000_0000`) and `VmHost::new_array` (`0x3000_0000`). The exe has one
/// heap and a plain pointer, so this decode disappears when the object model
/// unifies the ranges (M1-02 follow-up, outside this row).
fn object_index(word: u32) -> Option<usize> {
    let index = if word >= 0x3000_0000 {
        word - 0x3000_0000
    } else if word >= 0x2000_0000 {
        word - 0x2000_0000
    } else {
        word.checked_sub(0x1000)?
    };
    usize::try_from(index).ok()
}

/// The object's class name: the exe's `[[obj+0xc]+8]`, which is the class
/// descriptor's name cstring (`FUN_00404ea0`).
pub fn class_name(vm: &VmHost, word: u32) -> Option<&str> {
    let index = object_index(word)?;
    vm.objects.get(index).map(|o| o.class_name(&vm.loader))
}

/// The `toString` text: `sprintf(buf, "%s<%0x>", name, obj)`.
///
/// `%0x` is the CRT's lowercase-hex conversion with no width, so the `0` flag
/// has no effect (`0` -> `0`, `0xabcdef` -> `abcdef`; probed against the exe's
/// own `sprintf`).
#[must_use]
pub fn object_text(name: &str, word: u32) -> String {
    format!("{name}<{word:x}>")
}

/// Intern `bytes` and return the `java.lang.String` object word the exe's
/// `FUN_004174a0` returns.
///
/// Uses VmHost's shared interner and actual inherited Native.ptr field.
/// Required String metadata must be loaded; allocation does not invent it.
///
/// Shared with `java.lang.System.info` (M1-08 row O9), whose `FUN_004174a0`
/// call is the same interning path.
pub(crate) fn intern_string(vm: &mut VmHost, bytes: &[u8]) -> u32 {
    crate::seam::intern_string(vm, bytes)
}

/// `java.lang.Object.toString()Ljava.lang.String;`
///
/// exe: LAB_0047c870
pub fn to_string(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let word = receiver(info);
    // The exe reads `[[obj+0xc]+8]` directly off the receiver; an unknown
    // word dereferences unmapped memory. The port returns the null String
    // instead of inventing a class name.
    let name = vm
        .objects
        .get(object_index(word)?)
        .map(|o| o.class_name(&vm.loader).to_owned());
    let Some(name) = name else {
        return Some(0);
    };
    Some(intern_string(vm, object_text(&name, word).as_bytes()))
}

/// `java.lang.Object.disableGC()V`
///
/// exe: LAB_0047c8c0
pub fn disable_gc(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let word = receiver(info);
    if let Some(class) = class_name(vm, word) {
        let class = class.to_owned();
        with_gc_lists(|lists| lists.disable(&class, word));
    }
    None
}

/// `java.lang.Object.enableGC()V`
///
/// exe: LAB_0047c8e0
pub fn enable_gc(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let word = receiver(info);
    if let Some(class) = class_name(vm, word) {
        let class = class.to_owned();
        with_gc_lists(|lists| lists.enable(&class, word));
    }
    None
}

/// `java.lang.Object.hashCode()I`
///
/// The exe returns the receiver word itself (`eax = out[0]`): identity
/// hashing, no table.
///
/// exe: LAB_0047c900
pub fn hash_code(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    Some(receiver(info))
}

/// Install the four `java.lang.Object` ports into a dispatch table.
pub fn install(table: &mut NativeTable) {
    table.set_vm(TO_STRING, to_string);
    table.set_vm(DISABLE_GC, disable_gc);
    table.set_vm(ENABLE_GC, enable_gc);
    table.set_vm(HASH_CODE, hash_code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seam::{self, dispatch_on_vm, DispatchError};
    use slr_vm::host::CallInfo;

    fn info<'a>(args: &'a [Value], sig: &'a str, name: &'a str) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: OBJECT,
            name,
            sig,
        }
    }

    /// A host with one object of `class` in its heap; returns the object word.
    fn host_with_object(class: &str) -> (VmHost, u32) {
        let mut vm = crate::test_strings::vm();
        if vm.loader.lookup(class).is_none() {
            vm.loader.insert_test_class(slr_vm::vmclass::Class {
                kind: 0,
                name: class.into(),
                base: None,
                interfaces: vec![],
                methods: vec![],
                fields: vec![],
                bodies: vec![],
            });
        }
        let id = vm.loader.lookup(class).unwrap();
        let word = 0x1000 + vm.objects.len() as u32;
        vm.objects.push(Object {
            class: slr_vm::vmclass::ClassRef::Loaded(id),
            fields: Vec::new(),
        });
        (vm, word)
    }

    /// `%0x` is lowercase hex with no width: the CRT ignores the `0` flag
    /// without a width. Rows probed through the exe's own `sprintf` at
    /// `0x551220` (`Cls<0>`, `Cls<5>`, `Cls<abcdef>`, `Cls<200000c0>`).
    #[test]
    fn the_format_is_the_crt_percent_zero_x() {
        assert_eq!(object_text("Cls", 0), "Cls<0>");
        assert_eq!(object_text("Cls", 5), "Cls<5>");
        assert_eq!(object_text("Cls", 0xabcdef), "Cls<abcdef>");
        assert_eq!(object_text("Cls", 0x2000_00c0), "Cls<200000c0>");
        // `java.lang.Object` / `java.game.Car` rows from the Unicorn probe.
        assert_eq!(
            object_text("java.lang.Object", 0x2000_00c0),
            "java.lang.Object<200000c0>"
        );
        assert_eq!(
            object_text("java.game.Car", 0x2000_0230),
            "java.game.Car<20000230>"
        );
    }

    /// `toString` returns an interned `String` carrying the text; the exe
    /// interns through `FUN_004174a0` and returns the String object.
    #[test]
    fn to_string_interns_the_name_and_word() {
        let (mut vm, word) = host_with_object("java.game.Car");
        let args = [Value::reference(word)];
        let ret = to_string(&mut vm, &info(&args, "()Ljava.lang.String;", "toString"))
            .expect("an L return");
        assert_eq!(
            vm.strings
                .lookup(format!("java.game.Car<{word:x}>").as_bytes())
                .map(|r| r.bytes.clone()),
            Some(format!("java.game.Car<{word:x}>").into_bytes())
        );
        let index = (ret - 0x1000) as usize;
        assert_eq!(vm.objects[index].class_name(&vm.loader), "java.lang.String");
        assert_eq!(
            vm.string_bytes(ret).expect("actual String.ptr").len() as i32,
            19,
            "java.game.Car<1000>"
        );
    }

    /// A receiver word that names no object returns the null String (the exe
    /// faults on the descriptor dereference).
    #[test]
    fn to_string_of_an_unknown_word_is_null() {
        let mut vm = crate::test_strings::vm();
        let args = [Value::reference(0x1000 + 7)];
        assert_eq!(
            to_string(&mut vm, &info(&args, "()Ljava.lang.String;", "toString")),
            Some(0)
        );
        assert!(vm.strings.is_empty());
    }

    /// `hashCode` is the receiver word, and an empty container reads 0.
    #[test]
    fn hash_code_returns_the_receiver_word() {
        let mut vm = crate::test_strings::vm();
        let args = [Value::int(0xdead_beefu32 as i32)];
        assert_eq!(
            hash_code(&mut vm, &info(&args, "()I", "hashCode")),
            Some(0xdead_beef)
        );
        assert_eq!(hash_code(&mut vm, &info(&[], "()I", "hashCode")), Some(0));
    }

    /// A link value named by fixture identity, exactly like the oracle row
    /// records it (`o1`, `o2`, `hn` = normal head node, `hd` = disabled
    /// head node; either head node doubles as its list's head cell).
    fn rel(value: u32, o1: u32, o2: u32, hn: u32, hd: u32) -> String {
        if value == o1 {
            "o1".to_owned()
        } else if value == o2 {
            "o2".to_owned()
        } else if value == hn {
            "hn".to_owned()
        } else if value == hd {
            "hd".to_owned()
        } else {
            format!("{value:#x}")
        }
    }

    /// The class-context state after a step in the oracle row's shape:
    /// `norm` = (normal head cell, head node, its prev, its next),
    /// `dis` = the same four for the disabled list, `one`/`two` = each
    /// object's (next, prev, flags).
    fn snapshot(g: &ClassGc, o1: u32, o2: u32) -> [String; 4] {
        let (hn, hd) = (g.normal.sentinel, g.disabled.sentinel);
        let four = |cell: u32, node: u32| {
            format!(
                "{}|{}|{}|{}",
                rel(cell, o1, o2, hn, hd),
                rel(node, o1, o2, hn, hd),
                rel(g.node(node).prev, o1, o2, hn, hd),
                rel(g.node(node).next, o1, o2, hn, hd),
            )
        };
        let node = |w: u32| {
            format!(
                "{}|{}|{}",
                rel(g.node(w).next, o1, o2, hn, hd),
                rel(g.node(w).prev, o1, o2, hn, hd),
                g.node(w).flags,
            )
        };
        [
            four(g.normal.head, hn),
            four(g.disabled.head, hd),
            node(o1),
            node(o2),
        ]
    }

    /// The oracle's six-step `disableGC`/`enableGC` sequence
    /// (`re/oracle/langobj_vectors.py`, suite `gc`: disable o1, disable o2,
    /// enable o2, enable o1, disable o1, enable o2) against [`GcLists`]. The
    /// expected tuples are the oracle rows with the arena words replaced by
    /// fixture identity.
    #[test]
    fn gc_moves_reproduce_the_oracle_link_sequence() {
        reset();
        let (mut vm, o1) = host_with_object("java.game.Car");
        let o2 = 0x1000 + vm.objects.len() as u32;
        vm.objects.push(Object {
            class: slr_vm::vmclass::ClassRef::Loaded(vm.loader.lookup("java.game.Car").unwrap()),
            fields: Vec::new(),
        });
        let call = |vm: &mut VmHost, f: fn(&mut VmHost, &CallInfo<'_>) -> Option<u32>, w: u32| {
            let args = [Value::reference(w)];
            f(vm, &info(&args, "()V", "disableGC"))
        };
        let look = |o1: u32, o2: u32| {
            with_gc_lists(|l| {
                let g = l.class("java.game.Car").expect("class context");
                snapshot(g, o1, o2)
            })
        };

        // 1. disable(o1): inserted after the disabled head node.
        call(&mut vm, disable_gc, o1);
        assert_eq!(
            look(o1, o2),
            [
                "hn|hn|0x0|0x0".to_owned(),
                "o1|hd|0x0|o1".to_owned(),
                "0x0|hd|4".to_owned(),
                "0x0|0x0|0".to_owned(),
            ],
            "disable(o1)"
        );

        // 2. disable(o2): o1 plays the head node now.
        call(&mut vm, disable_gc, o2);
        assert_eq!(
            look(o1, o2),
            [
                "hn|hn|0x0|0x0".to_owned(),
                "o2|hd|0x0|o1".to_owned(),
                "o2|hd|4".to_owned(),
                "0x0|o1|4".to_owned(),
            ],
            "disable(o2)"
        );

        // 3. enable(o2): inserted before the normal head node; the disabled
        //    head cell keeps naming the object that left, and o1 still names
        //    it (an unlink only repairs when *both* links are set).
        call(&mut vm, enable_gc, o2);
        assert_eq!(
            look(o1, o2),
            [
                "o2|hn|o2|0x0".to_owned(),
                "o2|hd|0x0|o1".to_owned(),
                "o2|hd|4".to_owned(),
                "hn|0x0|0".to_owned(),
            ],
            "enable(o2)"
        );

        // 4. enable(o1): the normal head node is now o2, and the unlink
        //    rewrote o2.prev to o1's old prev before the insert overwrote it.
        call(&mut vm, enable_gc, o1);
        assert_eq!(
            look(o1, o2),
            [
                "o1|hn|o2|0x0".to_owned(),
                "o2|hd|0x0|o2".to_owned(),
                "o2|hd|0".to_owned(),
                "hn|o1|0".to_owned(),
            ],
            "enable(o1)"
        );

        // 5. disable(o1) again: unlinked from the normal list, re-inserted
        //    into the disabled one after the head node o2.
        call(&mut vm, disable_gc, o1);
        assert_eq!(
            look(o1, o2),
            [
                "o1|hn|o2|0x0".to_owned(),
                "o1|hd|0x0|o2".to_owned(),
                "hn|o2|4".to_owned(),
                "o1|hd|0".to_owned(),
            ],
            "disable(o1) again"
        );

        // 6. enable(o2) is a no-op: bit 4 is already clear.
        let before = look(o1, o2);
        call(&mut vm, enable_gc, o2);
        assert_eq!(look(o1, o2), before, "enable(o2) again");
        reset();
    }

    /// Both natives early-out on the flag they set: no relink, and the other
    /// flag bits survive.
    #[test]
    fn repeated_moves_are_no_ops() {
        let mut lists = GcLists::new();
        assert!(lists.disable("c", 0x1000));
        assert!(!lists.disable("c", 0x1000), "already disabled");
        let g = lists.class("c").expect("class context");
        assert_eq!(g.links(0x1000), (0, g.disabled.sentinel));
        assert_eq!(g.disabled.head, 0x1000);

        // `enable` clears bit 4 only: an object that was never disabled is
        // left alone, and a second `enable` is a no-op.
        let mut lists = GcLists::new();
        assert!(!lists.enable("c", 0x1000), "not disabled");
        assert_eq!(lists.class("c").expect("ctx").flags(0x1000), 0);
        lists.disable("c", 0x1000);
        assert!(lists.enable("c", 0x1000));
        assert!(!lists.enable("c", 0x1000));
        assert_eq!(lists.class("c").expect("ctx").flags(0x1000), 0);
    }

    /// A null receiver is a no-op (the exe dereferences it and faults).
    #[test]
    fn a_null_receiver_never_touches_the_lists() {
        reset();
        let mut vm = crate::test_strings::vm();
        let args = [Value::reference(0)];
        assert_eq!(disable_gc(&mut vm, &info(&args, "()V", "disableGC")), None);
        assert_eq!(enable_gc(&mut vm, &info(&args, "()V", "enableGC")), None);
        assert_eq!(with_gc_lists(|l| l.classes()), 0);
        reset();
    }

    /// The four rows register against the exe's own addresses, resolve
    /// through the VM registry, and dispatch through the seam's bridge with
    /// the object model in hand.
    #[test]
    fn the_four_rows_install_and_dispatch_through_the_bridge() {
        reset();
        seam::reset();
        let mut vm = crate::test_strings::vm();
        let car_id = vm.loader.lookup("java.game.Car").unwrap_or_else(|| {
            vm.loader.insert_test_class(slr_vm::vmclass::Class {
                kind: 0,
                name: "java.game.Car".into(),
                base: None,
                interfaces: vec![],
                methods: vec![],
                fields: vec![],
                bodies: vec![],
            })
        });
        let word = 0x1000 + vm.objects.len() as u32;
        vm.objects.push(Object {
            class: slr_vm::vmclass::ClassRef::Loaded(car_id),
            fields: Vec::new(),
        });
        let mut table = NativeTable::new();
        install(&mut table);
        assert_eq!(
            table.ported(),
            vec![TO_STRING, DISABLE_GC, ENABLE_GC, HASH_CODE]
        );
        seam::install(&mut vm, &table);

        for row in OBJECT_NATIVE_BINDINGS {
            assert_eq!(
                vm.natives.resolve(row.class, row.name, row.sig),
                Some(row.impl_addr),
                "{} {}{}",
                row.class,
                row.name,
                row.sig
            );
            assert!(vm.native_impls.contains_key(&row.impl_addr));
        }

        // `hashCode` end to end: `eax` is the receiver word (the oracle's
        // `0xdeadbeef` case).
        let args = [Value::int(0xdead_beefu32 as i32)];
        let h = seam::bridge(&mut vm, &info(&args, "()I", "hashCode"));
        assert_eq!(h, Some(0xdead_beef));

        // `toString` end to end through the bridge.
        let args = [Value::reference(word)];
        let s = seam::bridge(&mut vm, &info(&args, "()Ljava.lang.String;", "toString"))
            .expect("String word");
        assert_ne!(s, 0);
        assert!(vm
            .strings
            .lookup(format!("java.game.Car<{word:x}>").as_bytes())
            .is_some());

        // `disableGC` end to end, then the list state is visible.
        let args = [Value::reference(word)];
        assert_eq!(
            seam::bridge(&mut vm, &info(&args, "()V", "disableGC")),
            None
        );
        with_gc_lists(|l| {
            assert_eq!(
                l.class("java.game.Car").map(|g| g.flags(word)),
                Some(GC_DISABLED)
            );
        });
        reset();
        seam::reset();
    }

    /// A known-but-unported address and an unknown address stay distinct, and
    /// a VM-host port is not reachable through the lang-only entry.
    #[test]
    fn dispatch_reports_unported_and_unknown_addresses() {
        reset();
        let empty = NativeTable::new();
        let mut vm = crate::test_strings::vm();
        let empty_info = info(&[], "()I", "hashCode");
        assert_eq!(
            dispatch_on_vm(&empty, &mut vm, 0xdead_0001, &empty_info),
            Err(DispatchError::NoImplementation {
                impl_addr: 0xdead_0001
            })
        );
        assert_eq!(
            dispatch_on_vm(&empty, &mut vm, TO_STRING, &empty_info),
            Err(DispatchError::NotPorted {
                impl_addr: TO_STRING
            })
        );
        let mut host = crate::LangHost::new();
        assert_eq!(
            empty.dispatch(&mut host, HASH_CODE, &[]),
            Err(DispatchError::NotPorted {
                impl_addr: HASH_CODE
            })
        );

        // With the rows installed, the VM-host port runs and the lang-only
        // entry still refuses it (it has no object model to give).
        let mut table = NativeTable::new();
        install(&mut table);
        let args = [Value::int(0x1234_5678)];
        let args_info = info(&args, "()I", "hashCode");
        assert_eq!(
            dispatch_on_vm(&table, &mut vm, HASH_CODE, &args_info),
            Ok(Some(0x1234_5678))
        );
        assert_eq!(
            table.dispatch(&mut host, HASH_CODE, &args),
            Err(DispatchError::NotPorted {
                impl_addr: HASH_CODE
            })
        );
        reset();
    }
}
