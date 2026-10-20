//! Headless resource materialisation for `ResourceRef.load/cache/precache`.
//!
//! The original hands the container to the resource manager, which reads the
//! descriptor's source file and builds the engine object (a D3D texture, a
//! mesh, a sound buffer). The headless runtime has no device: it checks that
//! the descriptor's source file exists in the clean stock install, marks the
//! container loaded and records the request. Nothing is decoded and the file
//! reads of the original are not reproduced, so original file-I/O and D3D9
//! call-log records are absent from this stream (declared gap).
use slr_resource::node::with_res;
use slr_vm::host::{CallInfo, VmHost};
use std::{cell::RefCell, path::Path};

thread_local! {
    static TRACE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub fn take_trace() -> Vec<String> {
    TRACE.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

fn resolve_case_insensitive(root: &Path, relative: &str) -> Option<std::path::PathBuf> {
    let mut path = root.to_path_buf();
    for part in relative.split(['\\', '/']).filter(|p| !p.is_empty()) {
        let wanted = part.to_ascii_lowercase();
        let found = std::fs::read_dir(&path)
            .ok()?
            .filter_map(Result::ok)
            .find(|e| e.file_name().to_string_lossy().to_ascii_lowercase() == wanted)?;
        path = found.path();
    }
    Some(path)
}

/// Install over `ResourceRef.load/cache/precache`: the recorded effect first,
/// then the headless materialisation of the receiver's container.
pub fn load_like(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let receiver = info.args.last().map_or(0, |v| v.payload);
    let result = slr_resource::node::bridge(vm, info);
    let container = with_res(|h| {
        let node = h.object_node(receiver);
        (node != 0).then(|| h.node(node).map(|n| n.container))?
    })
    .filter(|c| *c != 0);
    if let Some(container) = container {
        if let Err(error) = materialise(container, info.name) {
            vm.native_failure = Some(format!(
                "{}.{}{}: ResourceMaterializationBoundary: {error}",
                info.class, info.name, info.sig
            ));
        }
    }
    result
}

fn materialise(container: u32, request: &str) -> Result<(), String> {
    let (id, type_id, loaded) = with_res(|h| {
        h.container(container)
            .map(|c| (c.id, c.type_id, c.load_state & 1 != 0))
    })
    .ok_or("container missing")?;
    if loaded {
        return Ok(());
    }
    let (descriptor, root) = crate::RES.with(|r| {
        let r = r.borrow();
        let package = r.packages.get((id >> 16) as usize).map(|(_, p)| p);
        let text = package.and_then(|p| {
            p.entries
                .iter()
                .find(|e| e.type_id == id & 0xffff)
                .map(|e| p.descriptor(e))
        });
        (text, r.root.clone())
    });
    // Containers the world made without a package entry (script instances,
    // the type-1 game nodes) have no descriptor to materialise.
    let Some(descriptor) = descriptor else {
        return Ok(());
    };
    let mut source = None;
    for line in descriptor.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(path) = line.strip_prefix("sourcefile ") {
            source = Some(path.trim().trim_end_matches('\0').to_owned());
        }
    }
    let mut note = format!("{request} id={id:#010x} type={type_id}");
    if let Some(source) = &source {
        let path = resolve_case_insensitive(&root, source)
            .ok_or_else(|| format!("sourcefile {source} is not in the stock install"))?;
        let size = std::fs::metadata(&path)
            .map_err(|e| format!("sourcefile {source}: {e}"))?
            .len();
        note.push_str(&format!(" sourcefile={source} bytes={size}"));
    } else {
        note.push_str(&format!(" descriptor={descriptor:?}"));
    }
    TRACE.with(|t| t.borrow_mut().push(note));
    with_res(|h| {
        if let Some(c) = h.container_mut(container) {
            c.load_state |= 1;
        }
    });
    Ok(())
}
