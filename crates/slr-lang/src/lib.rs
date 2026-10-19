//! `slr-lang` natives (language classes: Math, Float, Integer, String,
//! Vector3/Ypr, Object/System/Class — M1-06..M1-08).
//!
//! Slice layout (one handoff row each, see `slr-notes/handoff-m1-06.md`):
//!
//! - [`seam`] — the dispatch seam: [`LangHost`] state (CRT `rand` word, the
//!   `clock()` reading, the simulation's time word and the exit request), the
//!   [`LogSink`] console recorder, the [`AnalyzeSink`] `System.analyze`
//!   recorder, the `impl_addr -> LangNative` table, the bridge into
//!   [`slr_vm::host::VmHost::native_impls`], the 13 M1-06 registry rows read
//!   off the exe's native table, and the known rows of the other families
//!   ([`KNOWN_FAMILIES`]).
//! - [`langobj`] — the `java.lang.Object`/`System`/`GameType` natives
//!   (M1-08). [`langobj::object`] holds rows O1..O4 (toString, disableGC,
//!   enableGC, hashCode) and the GC-list model they run on;
//!   [`langobj::system`] holds rows O5 (`exit` ×2), O6 (`log`), O7
//!   (`buildNumber`, `currentTime`, `simTime`), O8 (the `analyze` trio),
//!   O9 (`info`) and O10 (`gc`, `runFinalization`).
//! - [`vec`] — the `java.lang.Vector3` / `java.lang.Ypr` value model and the
//!   eight registry rows for their natives; `Vector3.length` (`0x004823e0`),
//!   `Vector3.distance` (`0x00482550`), `Vector3.normalize` (`0x00482450`),
//!   `Vector3.rotate` (`0x00482110`, plus the shared ypr→3×3 helper
//!   `0x54ecd0`), `Vector3.rotate(Vector3,F)` (`0x00482280`, plus the
//!   axis-angle helper `0x54ee80`), `Vector3.<init>(Ypr)` (`0x00482020`),
//!   `Ypr.<init>(Vector3)` (`0x00482610`, plus the vec→ypr helper
//!   `0x551d20`) and `Ypr.mul` (`0x004826c0`, plus the mat3 multiply
//!   `0x54eab0` and the mat3→ypr helper `0x551c90`) are all ported and
//!   installed by [`vec::install`].
//! - [`math`] — the `java.lang.Math` ports that have landed (`randomize`,
//!   `LAB_0047c7d0`; `setrandseed`, `FUN_0047c7f0`; `random`, `LAB_0047c820`;
//!   `sqrt`, `LAB_0047c850`).
//! - [`format`] — the exe's mini-`sprintf` (`FUN_00553d70`) and its decimal
//!   renderer (`FUN_00554170`), shared by the `Integer`/`Float` `toString`
//!   natives ([`number`]).
//! - [`scan`] — the exe's mini-`sscanf` (`FUN_00553890`), the scanner behind
//!   the `String.intValue`/`floatValue` natives (row 7; no native is wired to
//!   it yet).
//! - [`number`] — the `java.lang.Integer`/`java.lang.Float` string
//!   conversions (`toString`, `LAB_00481e30`; `toHexString`,
//!   `LAB_00481e80`; `Float.toString`, `LAB_00481dd0`).
//! - [`thread`] — the `java.lang.Thread` / `java.lang.Object` thread and
//!   monitor natives (M1-05, one row each) over `slr_vm::threads` /
//!   `slr_vm::monitor`; all thirteen rows are ported (`0x47c510`..`0x47c970`).
//! - [`string`] — the `java.lang.String` family: `intValue` (`0x481ed0`),
//!   `floatValue` (`0x481f10`), `length` (`0x481fe0`), `append` (`0x481f50`),
//!   `finalize` (`0x486190`) and `token` (`0x481cf0`).
//!
//! Nothing here is called by the VM until a native is installed through
//! [`seam::install`] and [`seam::register`] runs (or [`thread::install`] for
//! the thread rows): an address with no ported implementation stays
//! `HostError::NoBody`, never a fabricated zero.

pub mod clock;
pub mod format;
pub mod langobj;
pub mod math;
pub mod number;
pub mod scan;
pub mod seam;
pub mod string;
pub mod thread;
pub mod vec;

pub use langobj::{object, system};
pub use seam::{
    bridge, dispatch, dispatch_on_vm, install, intern_string, known_rows, register, reset,
    with_analyze, with_lang_host, with_log, AnalyzeEvent, AnalyzeKind, AnalyzeSink, ConsoleAppend,
    DispatchError, LangHost, LangNative, LogSink, NativeBinding, NativeResult, NativeTable,
    SystemInfo, VmNative, KNOWN_FAMILIES, LANG_NATIVE_BINDINGS,
};

#[cfg(test)]
mod tests {
    use super::*;
    use slr_vm::host::VmHost;

    /// Every M1-06 row and every known M1-08 row the exe registers resolves
    /// back to its own implementation address after [`register`], and
    /// registering twice changes nothing (the exe's registrar is a plain
    /// overwrite).
    #[test]
    fn every_registered_row_resolves_to_its_impl_address() {
        let mut vm = VmHost::new();
        register(&mut vm);
        register(&mut vm);
        assert_eq!(
            vm.natives.len(),
            LANG_NATIVE_BINDINGS.len() + known_rows().count()
        );
        for row in LANG_NATIVE_BINDINGS.iter().chain(known_rows()) {
            assert_eq!(
                vm.natives.resolve(row.class, row.name, row.sig),
                Some(row.impl_addr),
                "{} {}{}",
                row.class,
                row.name,
                row.sig
            );
        }
    }

    /// An unregistered address is a typed error, not a zero result.
    #[test]
    fn unknown_address_is_a_dispatch_error() {
        let mut host = LangHost::new();
        let err = seam::NativeTable::new()
            .dispatch(&mut host, 0xdead_0001, &[])
            .unwrap_err();
        assert_eq!(
            err,
            DispatchError::NoImplementation {
                impl_addr: 0xdead_0001
            }
        );
    }
}

#[cfg(test)]
#[path = "../tests/support/string_vm.rs"]
pub(crate) mod test_strings;
