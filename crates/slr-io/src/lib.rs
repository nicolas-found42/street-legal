//! `slr-io` natives: File and FindFile over `stock/`; Input, MouseCursor, Controller logging stubs (M1-09).
//!
//! [`stock`] (M7-04) verifies the clean stock install by checksum at startup, replacing the
//! original's CD check (`FUN_00401480`, `texture.dat`).
//!
//! [`fileio`] holds the native file struct and the 32-entry file-slot table the `File` natives
//! sit on, [`path`] the game-path → DOS-path conversion every file-layer entry point performs,
//! [`file`] the `File.open` / `File.close` / `File.write` / `File.readInt` / `File.readFloat` /
//! `File.readString` natives over both, [`resid`] the `File.write(ResourceRef)` /
//! `File.readResID` pair over the two resource tables `close` writes into the SDAT footer,
//! [`findfile`] the `FindFile.first` / `next` / `close` trio over a port-side directory search,
//! and [`input`] the 20 `Input` / `MouseCursor` / `Controller` rows as logging stubs (they
//! read the DirectInput device table M6-04's `slr-input` will model; each stub records the
//! marshalled arguments and returns the constant the exe's device-less arm returns).

pub mod file;
pub mod fileio;
pub mod findfile;
pub mod input;
pub mod path;
pub mod resid;
pub mod stock;

pub use file::{
    bridge, install, register, reset, with_files, FileHost, FileObject, IoBinding, IoEvent,
    FILE_CLOSE, FILE_NATIVE_BINDINGS, FILE_OPEN, FILE_READ_FLOAT, FILE_READ_INT, FILE_READ_STRING,
    FILE_WRITE_FLOAT, FILE_WRITE_INT, FILE_WRITE_STRING, SDAT_HEADER, SDAT_VERSION,
};
pub use findfile::{
    accepts, is_dot_entry, pattern_match, reset_finds, split_pattern, with_finds, FindEntry,
    FindEvent, FindHost, FindObject, FindSlot, DIRS_ONLY, FIELD_DEFAULT, FILES_DIRS, FILES_ONLY,
    FIND_CLOSE, FIND_FILE_CLASS, FIND_FIRST, FIND_NATIVE_BINDINGS, FIND_NEXT, FIND_NOT_FOUND,
    FIND_NO_SLOT, FIND_SLOTS,
};
pub use input::{
    all_stubs, log_value, param_types, reset_input, return_type, stub_by_addr, with_input,
    InputHost, Stub, StubResult, CONTROLLER_CLASS, CONTROLLER_DESCRIPTOR, CONTROLLER_STUBS,
    CONTROLLER_USER_ADD, CONTROLLER_USER_DEL, CONTROLLER_USER_GET_AXIS_VAL, CONTROLLER_USER_RESET,
    CONTROLLER_USER_SET_AXIS_FORCE, CONTROLLER_USER_SET_AXIS_SMOOTH,
    CONTROLLER_USER_SET_AXIS_SPEED, CURSOR_GET_PICKED_POS, CURSOR_GET_POS, DEVICE_COUNT,
    DEVICE_RECORDS, DEVICE_STRIDE, DEVICE_TABLE, INPUT_ACTIVE_AXIS, INPUT_AXIS_NAME,
    INPUT_CHECK_HOTKEYS, INPUT_CLASS, INPUT_CREATE_HOTKEY, INPUT_DELETE_HOTKEY,
    INPUT_FLUSH_HOTKEYS, INPUT_GET_AXIS, INPUT_GET_DEVICE_NAME, INPUT_LAST_KEY, INPUT_MAP_AXIS,
    INPUT_SET_AXIS_SMOOTH, INPUT_STUBS, INPUT_ZERO_FLOAT, LOG_SINK, MAX_WORDS, MOUSE_CURSOR_CLASS,
    MOUSE_CURSOR_STUBS, MOUSE_ERROR, MOUSE_ERROR_2,
};
pub use input::{bridge as input_bridge, install as install_input, register as register_input};
pub use resid::{
    ResourceRow, SlotRow, FILE_READ_RES_ID, FILE_WRITE_RES_REF, RESID_NATIVE_BINDINGS,
    UNSAVEABLE_MESSAGE, UNSAVED_MESSAGE,
};
