//! Original notification records and their two intrusive list memberships.
//! Resource loading is supplied by the caller; list mutation and weak links
//! execute here, including retirement rather than immediate destruction.
use slr_physics::space::Space;

const RETIRED: u32 = 0x6363e0;

pub trait Host {
    fn access(&mut self, memory: &mut Space, resource: u32) -> Result<u32, String>;
    fn free(&mut self, address: u32);
}

/// The context's original428fd0 list constructor.
pub fn initialize_retired(m: &mut Space) {
    for (offset, word) in [
        (0, 0x5f09a8),
        (4, 0x5f09ac),
        (8, RETIRED + 16),
        (12, 0),
        (16, 0x5f09ac),
        (20, 0),
        (24, RETIRED + 4),
    ] {
        m.w32(RETIRED + offset, word);
    }
}

pub fn first(m: &Space, envelope: u32) -> u32 {
    let node = m.r32(envelope + 0x5c);
    if m.r32(node + 4) == 0 {
        0
    } else {
        node
    }
}

pub fn next(m: &Space, node: u32) -> u32 {
    let node = m.r32(node + 4);
    if node == 0 || m.r32(node + 4) == 0 {
        0
    } else {
        node
    }
}

fn find(m: &Space, envelope: u32, listener_id: u32, mask: u32) -> u32 {
    let mut node = first(m, envelope);
    while node != 0 {
        if m.r32(node + 0x14) == listener_id && m.r32(node + 0x1c) == mask {
            return node;
        }
        node = next(m, node);
    }
    0
}

fn unlink(m: &mut Space, node: u32) {
    let previous = m.r32(node + 8);
    let next = m.r32(node + 4);
    if previous != 0 && next != 0 {
        m.w32(next + 8, previous);
        m.w32(previous + 4, next);
    }
    m.w32(node + 4, 0);
    m.w32(node + 8, 0);
}

fn retire(m: &mut Space, node: u32) {
    // The unbound-reference branch clears only the id, retaining the other
    // reference words just as48b500/48b670 do.
    if m.r32(node + 0x18) == 0 {
        m.w32(node + 0x14, 0);
    } else {
        crate::native_part::bind_reference(m, node + 12, 0);
    }
    unlink(m, node);
    let tail = m.r32(RETIRED + 24);
    m.w32(tail + 4, node);
    m.w32(node + 4, RETIRED + 16);
    m.w32(node + 8, tail);
    m.w32(RETIRED + 24, node);
}

fn envelope(m: &mut Space, reference: u32, host: &mut dyn Host) -> Result<u32, String> {
    if m.r32(reference + 8) == 0 || m.r32(reference + 12) == 0 {
        return Ok(0);
    }
    host.access(m, m.r32(reference + 12))
}

/// exe: FUN_0048b500
#[allow(clippy::too_many_arguments)] // Original thiscall arguments.
pub fn add(
    m: &mut Space,
    source: u32,
    listener: u32,
    mask: u32,
    kind: u32,
    argument: u32,
    name: Option<&[u8]>,
    host: &mut dyn Host,
) -> Result<u32, String> {
    let data = envelope(m, source, host)?;
    if data == 0 {
        return Ok(0);
    }
    let old = find(m, data, m.r32(listener + 8), mask);
    if old != 0 {
        retire(m, old);
    }
    let node = m.malloc(0x2c);
    m.w32(node, 0x5f13d8);
    m.w32(node + 4, 0);
    m.w32(node + 8, 0);
    for offset in (12..28).step_by(4) {
        m.w32(node + offset, 0);
    }
    crate::native_part::bind_reference(m, node + 12, m.r32(listener + 12));
    m.w32(node + 0x1c, mask);
    m.w32(node + 0x20, kind);
    m.w32(node + 0x24, argument);
    let name_pointer = if let Some(name) = name {
        let length = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        let pointer = m.malloc(length as u32 + 1);
        //551120 ->5548c0 copies at most255 bytes and terminates them.
        for (i, &byte) in name[..length].iter().take(255).enumerate() {
            m.w8(pointer + i as u32, byte);
        }
        m.w8(pointer + length.min(255) as u32, 0);
        pointer
    } else {
        0
    };
    m.w32(node + 0x28, name_pointer);
    let old_first = m.r32(data + 0x5c);
    m.w32(old_first + 8, node);
    m.w32(node + 4, old_first);
    m.w32(node + 8, data + 0x58);
    m.w32(data + 0x5c, node);
    Ok(1)
}

/// exe: FUN_0048b670
pub fn remove(
    m: &mut Space,
    source: u32,
    listener: u32,
    mask: u32,
    host: &mut dyn Host,
) -> Result<u32, String> {
    let data = envelope(m, source, host)?;
    if data == 0 {
        return Ok(0);
    }
    let old = find(m, data, m.r32(listener + 8), mask);
    if old == 0 {
        return Ok(0);
    }
    retire(m, old);
    Ok(1)
}

/// exe: FUN_0048f290
pub fn destroy(m: &mut Space, node: u32, flags: u32, host: &mut dyn Host) -> u32 {
    m.w32(node, 0x5f13d8);
    if m.r32(node + 0x20) & 0x20000000 != 0 {
        host.free(m.r32(node + 0x24));
    }
    if m.r32(node + 0x28) != 0 {
        host.free(m.r32(node + 0x28));
    }
    if m.r32(node + 0x18) == 0 {
        m.w32(node + 0x14, 0);
    } else {
        crate::native_part::bind_reference(m, node + 12, 0);
    }
    m.w32(node, 0x5f09ac);
    unlink(m, node);
    if flags & 1 != 0 {
        host.free(node);
    }
    node
}

/// Production resource access keeps the same419860 flags and quality.
/// Space is an arena: freeing drops ownership without reusing addresses.
pub struct Runtime;
impl Host for Runtime {
    fn access(&mut self, m: &mut Space, resource: u32) -> Result<u32, String> {
        crate::native_assets::access(
            m,
            resource,
            crate::native_resource_load::Access {
                flags: 0x80000000,
                quality: 1f32.to_bits(),
                priority: 0,
                time: 0,
            },
        )
    }
    fn free(&mut self, _address: u32) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Controlled {
        data: u32,
        loaded: bool,
        calls: Vec<Value>,
        freed: Vec<u32>,
    }
    impl Host for Controlled {
        fn access(&mut self, _: &mut Space, resource: u32) -> Result<u32, String> {
            self.calls
                .push(json!({"this":resource,"args":[0x80000000u32,1f32.to_bits(),0,0]}));
            Ok(if self.loaded { self.data } else { 0 })
        }
        fn free(&mut self, address: u32) {
            self.freed.push(address);
        }
    }
    #[test]
    fn original_notification_lists_and_destructors_match_complete_memory() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/oracle/notification-lists.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/notification_list_probe.py");
            return;
        };
        let fixture: Value = serde_json::from_slice(&bytes).unwrap();
        let word = |v: &Value| v.as_u64().unwrap() as u32;
        for (index, row) in fixture["rows"].as_array().unwrap().iter().enumerate() {
            let mut m = Space::new();
            let before = row["before"].as_array().unwrap();
            for (i, v) in before.iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, word(v));
            }
            m.set_heap_ptr(0x20000000 + before.len() as u32 * 4);
            initialize_retired(&mut m);
            let source = word(&row["source"]);
            let listener = word(&row["listener"]);
            let data = word(&row["data"]);
            let mut host = Controlled {
                data,
                loaded: row["case"]["loaded"].as_bool().unwrap(),
                calls: vec![],
                freed: vec![],
            };
            let mut results = vec![];
            for op in row["case"]["ops"].as_array().unwrap() {
                let target = if op["linked"].as_bool().unwrap_or(true) {
                    word(&row["targets"][op["target"].as_u64().unwrap_or(0) as usize])
                } else {
                    0
                };
                crate::native_part::bind_reference(&mut m, listener, target);
                if target == 0 {
                    m.w32(listener + 8, op["unbound_id"].as_u64().unwrap_or(0) as u32);
                }
                results.push(match op["op"].as_str().unwrap() {
                    "add" => add(
                        &mut m,
                        source,
                        listener,
                        word(&op["mask"]),
                        op["kind"].as_u64().unwrap_or(0) as u32,
                        op["arg"].as_u64().unwrap_or(0) as u32,
                        op["name"].as_str().map(str::as_bytes),
                        &mut host,
                    )
                    .unwrap(),
                    "remove" => {
                        remove(&mut m, source, listener, word(&op["mask"]), &mut host).unwrap()
                    }
                    "destroy" => {
                        let node = m.r32(data + 0x5c);
                        destroy(&mut m, node, word(&op["flags"]), &mut host)
                    }
                    op => panic!("unknown fixture operation {op}"),
                });
            }
            assert_eq!(json!(results), row["results"], "case{index} results");
            assert_eq!(json!(host.calls), row["calls"], "case{index} access calls");
            assert_eq!(json!(host.freed), row["freed"], "case{index} free order");
            assert_eq!(m.heap_ptr(), word(&row["heap"]), "case{index} allocator");
            for (i, v) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    word(v),
                    "case{index} heap word{i}"
                );
            }
            for (i, v) in row["retired"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(RETIRED + i as u32 * 4),
                    word(v),
                    "case{index} retired word{i}"
                );
            }
        }
        assert_eq!(fixture["rows"].as_array().unwrap().len(), 58);
    }
}
