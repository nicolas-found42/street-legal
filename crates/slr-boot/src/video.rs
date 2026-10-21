//! Original GfxEngine.openVideo's null, disabled and COM-failure paths.
//!
//! The declared headless machine does not register DirectShow's FilterGraph
//! COM class (the same external availability as the original platform profile).
//! Original47c330/4f8dd0 guards and stores execute here; successful graph/media
//! playback is outside this projection. No native return fixture is installed.
use slr_vm::host::{marshal_args, CallInfo, VmHost};
use std::cell::RefCell;

pub const OPEN_VIDEO: u32 = 0x0047_c330;
const E_FAIL: u32 = 0x8000_4005;

#[derive(Clone, Copy, Debug)]
pub enum FilterGraphAvailability {
    Unregistered,
    ComNotInitialized,
}
impl FilterGraphAvailability {
    fn failure(self) -> u32 {
        match self {
            Self::Unregistered => 0x8004_0154,
            Self::ComNotInitialized => 0x8004_01f0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VideoPlatform {
    pub enabled: bool,
    pub filter_graph: FilterGraphAvailability,
}
impl VideoPlatform {
    /// `re/headless/make_boot_profile.py`: FilterGraph CLSID
    /// e436ebb3-524f-11ce-9f53-0020af0ba770 is explicitly unregistered.
    pub const fn declared_headless() -> Self {
        Self::unregistered_filter_graph(true, true)
    }
    pub const fn unregistered_filter_graph(enabled: bool, com_initialized: bool) -> Self {
        Self {
            enabled,
            filter_graph: if com_initialized {
                FilterGraphAvailability::Unregistered
            } else {
                FilterGraphAvailability::ComNotInitialized
            },
        }
    }
}

#[derive(Debug)]
struct VideoState {
    platform: VideoPlatform,
    filename: Vec<u8>,
    windowed: u32,
    graph: u32,
    playing: u32,
    create_attempts: u64,
}
impl VideoState {
    fn new(platform: VideoPlatform) -> Self {
        Self {
            platform,
            filename: vec![],
            windowed: 0,
            graph: 0,
            playing: 0,
            create_attempts: 0,
        }
    }
    fn open(&mut self, filename: Option<&[u8]>, windowed: u32) -> Result<u32, String> {
        // Wrapper47c355 checks the marshalled pointer before engine dispatch.
        let Some(filename) = filename else {
            return Ok(u32::MAX);
        };
        if !self.platform.enabled {
            return Ok(E_FAIL);
        }
        if self.playing != 0 {
            return Err(
                "VideoGraphBoundary: closing an active media graph is not implemented".into(),
            );
        }
        // 4f8dd0 copies the C filename and stores61852c before calling COM.
        let end = filename
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(filename.len());
        self.filename.clear();
        self.filename.extend_from_slice(&filename[..end]);
        self.windowed = windowed;
        self.create_attempts += 1;
        // CoCreateInstance clears its output pointer on failure. The original
        // immediately returns that failing HRESULT; it never sets playing.
        self.graph = 0;
        Ok(self.platform.filter_graph.failure())
    }
}
thread_local! {
    static STATE: RefCell<VideoState> = RefCell::new(VideoState::new(VideoPlatform::declared_headless()));
}

pub fn configure(platform: VideoPlatform) {
    STATE.with(|state| *state.borrow_mut() = VideoState::new(platform));
    slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
        category: "video-platform".into(),
        fields: vec![
            ("enabled".into(), platform.enabled.to_string()),
            (
                "filter_graph_availability".into(),
                format!("{:?}", platform.filter_graph),
            ),
        ],
    });
}

pub fn install(vm: &mut VmHost) {
    configure(VideoPlatform::declared_headless());
    vm.native_impls.insert(OPEN_VIDEO, open_video);
}

fn open_video(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut words = [0; 4];
    if let Err(error) = marshal_args(info, &mut words) {
        vm.native_failure = Some(format!("VideoArgumentBoundary: {error:?}"));
        return None;
    }
    let filename = if words[1] == 0 {
        None
    } else {
        match vm.string_bytes(words[1]) {
            Some(bytes) => Some(bytes),
            None => {
                vm.native_failure = Some(format!(
                    "VideoArgumentBoundary: filename {:#x} has no String bytes",
                    words[1]
                ));
                return None;
            }
        }
    };
    let result = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let result = state.open(filename, words[2]);
        slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
            category: "video-open".into(),
            fields: vec![
                (
                    "filename_bytes".into(),
                    serde_json::to_string(&state.filename).unwrap(),
                ),
                ("windowed".into(), state.windowed.to_string()),
                ("repeat_argument".into(), words[3].to_string()),
                (
                    "com_create_attempts".into(),
                    state.create_attempts.to_string(),
                ),
                ("graph".into(), state.graph.to_string()),
                ("playing".into(), state.playing.to_string()),
                ("result".into(), format!("{result:?}")),
            ],
        });
        result
    });
    match result {
        Ok(word) => Some(word),
        Err(error) => {
            vm.native_failure = Some(error);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slr_vm::values::Value;

    #[test]
    fn original_native_rows_match_guards_com_failure_and_state_stores() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/video-open-original.jsonl");
        let rows = std::fs::read_to_string(path).expect("original video probe rows required");
        for line in rows.lines() {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let availability = match row["com_hresult"].as_u64().unwrap() as u32 {
                0x8004_0154 => FilterGraphAvailability::Unregistered,
                0x8004_01f0 => FilterGraphAvailability::ComNotInitialized,
                other => panic!("unsupported external COM case {other:#x}"),
            };
            let mut state = VideoState::new(VideoPlatform {
                enabled: row["enabled"] == 1,
                filter_graph: availability,
            });
            state.graph = 0x1234_5678;
            state.windowed = 0x1111_1111;
            let result = state
                .open(
                    row["filename"].as_str().map(str::as_bytes),
                    row["windowed"].as_u64().unwrap() as u32,
                )
                .unwrap();
            assert_eq!(result as u64, row["result"].as_u64().unwrap());
            assert_eq!(
                state.filename,
                row["filename_after"].as_str().unwrap().as_bytes()
            );
            assert_eq!(
                state.windowed as u64,
                row["windowed_after"].as_u64().unwrap()
            );
            assert_eq!(state.graph as u64, row["graph_after"].as_u64().unwrap());
            assert_eq!(state.playing as u64, row["playing"].as_u64().unwrap());
            assert_eq!(
                state.create_attempts as usize,
                row["calls"].as_array().unwrap().len()
            );
        }
    }

    #[test]
    fn vm_filename_bytes_drive_failure_state_without_starting_playback() {
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(slr_vm::vmclass::Class {
            kind: 0,
            name: "java.lang.String".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            bodies: vec![],
            fields: vec![slr_vm::vmclass::Field {
                class: "java.lang.String".into(),
                name: "ptr".into(),
                ty: "I".into(),
                is_static: false,
            }],
        });
        install(&mut vm);
        let name = vm
            .intern_string(Some(b"data\\fmv\\prime.avi\0ignored"))
            .unwrap();
        let before = vm.objects.len();
        let args = [
            Value::int(1),
            Value::int(1),
            Value::reference_named(name, "Ljava.lang.String;"),
            Value::reference(0),
        ];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: "java.render.GfxEngine",
            name: "openVideo",
            sig: "(Ljava.lang.String;II)I",
        };
        assert_eq!(open_video(&mut vm, &info), Some(0x8004_0154));
        assert!(vm.native_failure.is_none());
        assert_eq!(vm.objects.len(), before);
        STATE.with(|state| {
            let state = state.borrow();
            assert_eq!(state.filename, b"data\\fmv\\prime.avi");
            assert_eq!(state.windowed, 1);
            assert_eq!(state.create_attempts, 1);
            assert_eq!(state.graph, 0);
            assert_eq!(state.playing, 0);
        });
    }

    #[test]
    fn vm_null_filename_preserves_state_without_allocations_or_com_request() {
        let mut vm = VmHost::new();
        install(&mut vm);
        let args = [
            Value::int(1),
            Value::int(1),
            Value::reference_named(0, "Ljava.lang.String;"),
            Value::reference(0),
        ];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: "java.render.GfxEngine",
            name: "openVideo",
            sig: "(Ljava.lang.String;II)I",
        };
        assert_eq!(open_video(&mut vm, &info), Some(u32::MAX));
        assert!(vm.native_failure.is_none());
        assert!(vm.objects.is_empty());
        STATE.with(|state| {
            let state = state.borrow();
            assert_eq!(state.create_attempts, 0);
            assert_eq!(state.playing, 0);
            assert!(state.filename.is_empty());
        });
    }
}
