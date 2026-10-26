//! Complete original4265c0 event routing over live resource collaborators.
//!
//! Class handlers, script execution and notification destruction remain host
//! calls: each can mutate the memory read by the next executable instruction.

pub trait Host {
    fn read(&mut self, address: u32) -> Result<u32, String>;
    fn write(&mut self, address: u32, value: u32) -> Result<(), String>;
    fn scratch_reference(&mut self) -> Result<u32, String>;
    fn release_reference(&mut self, reference: u32) -> Result<(), String>;
    fn minimum(&mut self, resource: u32) -> Result<(), String>;
    fn prepare(&mut self, resource: u32, flags: u32) -> Result<u32, String>;
    fn data(&mut self, resource: u32) -> Result<u32, String>;
    fn native(
        &mut self,
        payload: u32,
        body: u32,
        source: u32,
        event: u32,
        argument: u32,
    ) -> Result<u32, String>;
    #[allow(clippy::too_many_arguments)] // Original425c60 cdecl arguments plus host.
    fn script(
        &mut self,
        reference: u32,
        source: u32,
        event: u32,
        argument: u32,
        envelope: u32,
        name: u32,
    ) -> Result<(), String>;
    fn destroy_notification(&mut self, node: u32) -> Result<(), String>;
}

/// Live collaborators used by original4266ea's notification half.
pub trait NotificationHost {
    fn read(&mut self, address: u32) -> Result<u32, String>;
    fn forward(
        &mut self,
        reference: u32,
        source: u32,
        event: u32,
        argument: u32,
        name: u32,
    ) -> Result<(), String>;
    fn destroy_notification(&mut self, node: u32) -> Result<(), String>;
}

/// Original4266ea..4267b9. `origin` is this dispatch's retained weak reference;
/// it becomes the source for a chain started with `source == 0`.
pub fn walk_notifications(
    host: &mut dyn NotificationHost,
    envelope: u32,
    source: u32,
    event: u32,
    argument: u32,
    origin: u32,
) -> Result<(), String> {
    let mut node = host.read(envelope + 0x5c)?;
    // +5c is always a valid list endpoint in a constructed game envelope.
    if host.read(node + 4)? == 0 {
        return Ok(());
    }
    while node != 0 {
        // The original snapshots next before calling any listener. Later
        // nodes' content is read live when that listener has returned.
        let next = host.read(node + 4)?;
        let next = if next != 0 && host.read(next + 4)? != 0 {
            next
        } else {
            0
        };
        if host.read(node + 0x14)? == 0 {
            host.destroy_notification(node)?;
            node = next;
            continue;
        }
        let mask = host.read(node + 0x1c)?;
        if event & mask != 0 {
            let kind = host.read(node + 0x20)?;
            let forwarded = if kind & 0x0fffffff != 0 { kind } else { event };
            let forwarded_argument = if kind & 0x10000000 != 0 {
                host.read(node + 0x24)?
            } else {
                argument
            };
            if source == 0 || mask & 0x40000000 == 0 {
                let forwarded_name = host.read(node + 0x28)?;
                host.forward(
                    node + 12,
                    if source == 0 { origin } else { source },
                    forwarded & 0x0fffffff,
                    forwarded_argument,
                    forwarded_name,
                )?;
            }
            // A callback can change this mask; the exe rereads it.
            if host.read(node + 0x1c)? & 0x80000000 != 0 {
                host.destroy_notification(node)?;
            }
        }
        node = next;
    }
    Ok(())
}

struct NotificationAdapter<'a>(&'a mut dyn Host);
impl NotificationHost for NotificationAdapter<'_> {
    fn read(&mut self, address: u32) -> Result<u32, String> {
        self.0.read(address)
    }
    fn forward(
        &mut self,
        reference: u32,
        source: u32,
        event: u32,
        argument: u32,
        name: u32,
    ) -> Result<(), String> {
        dispatch(self.0, reference, source, event, argument, name).map(|_| ())
    }
    fn destroy_notification(&mut self, node: u32) -> Result<(), String> {
        self.0.destroy_notification(node)
    }
}

/// Original429060 binding, including intrusive weak-reference order.
fn bind(host: &mut dyn Host, reference: u32, resource: u32) -> Result<(), String> {
    if host.read(reference + 12)? == resource {
        return Ok(());
    }
    unlink(host, reference)?;
    for offset in [0, 4, 8, 12] {
        host.write(reference + offset, 0)?;
    }
    if resource != 0 {
        let next = host.read(resource + 0x48)?;
        if next != 0 {
            host.write(next, reference)?;
        }
        host.write(reference, 0)?;
        host.write(reference + 4, next)?;
        host.write(resource + 0x48, reference)?;
        host.write(reference + 12, resource)?;
        let id = host.read(resource + 0x50)?;
        host.write(reference + 8, id)?;
    }
    Ok(())
}

/// Original4267c2 cleanup only unlinks; the stack reference is then discarded.
fn unlink(host: &mut dyn Host, reference: u32) -> Result<(), String> {
    let resource = host.read(reference + 12)?;
    if resource != 0 {
        let previous = host.read(reference)?;
        let next = host.read(reference + 4)?;
        host.write(
            if previous == 0 {
                resource + 0x48
            } else {
                previous + 4
            },
            next,
        )?;
        if next != 0 {
            host.write(next, previous)?;
        }
    }
    Ok(())
}

/// All branches including live notification-list traversal.
/// exe: FUN_004265c0
/// `source` is a weak resource reference address, or zero for a fresh event.
/// Every valid original path returns one, even if no class/script handles it.
pub fn dispatch(
    host: &mut dyn Host,
    reference: u32,
    source: u32,
    event: u32,
    argument: u32,
    name: u32,
) -> Result<u32, String> {
    let local = host.scratch_reference()?;
    for offset in [0, 4, 8, 12] {
        host.write(local + offset, 0)?;
    }
    let result = (|| {
        let resource = host.read(reference + 12)?;
        bind(host, local, resource)?;
        let resource = host.read(local + 12)?;
        if resource == 0 || host.read(resource + 0x4c)? != 1 {
            return Ok(1);
        }
        if host.prepare(resource, 0xa0000000)? & 0x80000000 != 0 {
            return Ok(1);
        }
        let envelope = host.data(resource)?;
        if envelope == 0 {
            return Ok(1);
        }
        if host.read(envelope + 0x70)? & event != 0 {
            let mut consumed = false;
            if host.read(envelope + 0x4c)? != 0 {
                let secondary = host.read(envelope + 0x44)?;
                if secondary != 0 {
                    if host.read(secondary + 0x4c)? != 1 {
                        host.minimum(secondary)?;
                    }
                    if host.prepare(secondary, 0x80000000)? & 0x80000000 == 0 {
                        let data = host.data(secondary)?;
                        if data != 0 {
                            let payload = host.read(data + 12)?;
                            let body = host.read(envelope + 0x4c)?;
                            consumed =
                                host.native(payload, body, source, event & 0x0fffffff, argument)?
                                    != 0;
                        }
                    }
                }
            }
            if !consumed && host.read(envelope + 0x50)? != 0 {
                host.script(reference, source, event, argument, envelope, name)?;
            }
        }
        walk_notifications(
            &mut NotificationAdapter(host),
            envelope,
            source,
            event,
            argument,
            local,
        )?;
        Ok(1)
    })();
    let cleanup = unlink(host, local);
    let release = host.release_reference(local);
    result.and(cleanup).and(release).map(|()| 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use slr_physics::space::Space;

    struct Probe {
        memory: Space,
        row: Value,
        calls: Vec<Value>,
        depth: usize,
    }
    impl Probe {
        fn name(&self, name: &str) -> u32 {
            self.row["names"][name].as_u64().unwrap() as u32
        }
        fn value(&self, name: &str, default: u32) -> u32 {
            self.row["case"][name]
                .as_u64()
                .map_or(default, |v| v as u32)
        }
        fn record(&mut self, kind: &str, this: u32, args: &[u32]) {
            self.calls
                .push(json!({"kind":kind,"this":this,"args":args}));
        }
        fn mutation(&mut self, kind: &str, index: u32) {
            if self.row["case"]["mutate_on"] != json!([kind, index]) {
                return;
            }
            let envelope = self.name(&format!("envelope{index}"));
            match self.row["case"]["mutation"].as_str().unwrap() {
                "clear_script" => self.memory.w32(envelope + 0x50, 0),
                "set_script" => self.memory.w32(envelope + 0x50, 0xabc),
                "clear_body" => self.memory.w32(envelope + 0x4c, 0),
                "clear_next_listener" => self.memory.w32(self.name("node1") + 0x14, 0),
                "rewrite_next_kind" => {
                    self.memory.w32(self.name("node1") + 0x20, 0x10000080);
                    self.memory.w32(self.name("node1") + 0x24, 0x123456);
                }
                "replace_list" => self
                    .memory
                    .w32(self.name("envelope0") + 0x5c, self.name("node1")),
                "sever_current_next" => self
                    .memory
                    .w32(self.name("node0") + 4, self.name("sentinel0")),
                "clear_one_shot" => self.memory.w32(self.name("node0") + 0x1c, 0x10),
                "set_one_shot" => self.memory.w32(self.name("node0") + 0x1c, 0x80000010),
                other => panic!("unknown mutation {other}"),
            }
        }
        fn resource_index(&self, resource: u32) -> u32 {
            (0..4)
                .find(|i| {
                    self.name(&format!("resource{i}")) == resource
                        || self.name(&format!("secondary{i}")) == resource
                })
                .unwrap()
        }
    }
    impl Host for Probe {
        fn read(&mut self, address: u32) -> Result<u32, String> {
            Ok(self.memory.r32(address))
        }
        fn write(&mut self, address: u32, value: u32) -> Result<(), String> {
            self.memory.w32(address, value);
            Ok(())
        }
        fn scratch_reference(&mut self) -> Result<u32, String> {
            let address = self.row["scratch_references"][self.depth].as_u64().unwrap() as u32;
            self.depth += 1;
            Ok(address)
        }
        fn release_reference(&mut self, _reference: u32) -> Result<(), String> {
            self.depth -= 1;
            Ok(())
        }
        fn minimum(&mut self, resource: u32) -> Result<(), String> {
            self.record("minimum", resource, &[1f32.to_bits()]);
            Ok(())
        }
        fn prepare(&mut self, resource: u32, flags: u32) -> Result<u32, String> {
            self.record("prepare", resource, &[flags, 0, 0]);
            self.mutation("prepare", self.resource_index(resource));
            Ok(self.value(
                if resource == self.name("resource0") {
                    "flags"
                } else {
                    "secondary_flags"
                },
                0,
            ))
        }
        fn data(&mut self, resource: u32) -> Result<u32, String> {
            self.record("data", resource, &[1f32.to_bits()]);
            let index = self.resource_index(resource);
            let secondary = resource == self.name(&format!("secondary{index}"));
            self.mutation("data", index);
            if secondary {
                self.mutation("secondary_data", index);
            }
            let resource_name = format!(
                "{}{index}",
                if secondary { "secondary" } else { "resource" }
            );
            if self.row["case"]["null_data"].as_str() == Some(&resource_name) {
                return Ok(0);
            }
            Ok(self.name(&format!(
                "{}{index}",
                if secondary { "data" } else { "envelope" }
            )))
        }
        fn native(
            &mut self,
            payload: u32,
            body: u32,
            source: u32,
            event: u32,
            argument: u32,
        ) -> Result<u32, String> {
            self.record("native", payload, &[body, source, event, argument]);
            let index = (0..4)
                .find(|i| self.name(&format!("payload{i}")) == payload)
                .unwrap();
            self.mutation("native", index);
            Ok(if index == 0 {
                self.value("consumed", 0)
            } else {
                0
            })
        }
        fn script(
            &mut self,
            reference: u32,
            source: u32,
            event: u32,
            argument: u32,
            envelope: u32,
            name: u32,
        ) -> Result<(), String> {
            self.record(
                "script",
                0,
                &[reference, source, event, argument, envelope, name],
            );
            let index = (0..4)
                .find(|i| self.name(&format!("envelope{i}")) == envelope)
                .unwrap();
            self.mutation("script", index);
            Ok(())
        }
        fn destroy_notification(&mut self, node: u32) -> Result<(), String> {
            self.record("destroy", node, &[1]);
            let next = self.memory.r32(node + 4);
            let previous = self.memory.r32(node + 8);
            if next != 0 && previous != 0 {
                self.memory.w32(next + 8, previous);
                self.memory.w32(previous + 4, next);
            }
            self.memory.w32(node + 4, 0);
            self.memory.w32(node + 8, 0);
            bind(self, node + 12, 0)
        }
    }

    #[test]
    fn original4265c0_all_routes_and_live_callback_mutations() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/oracle/command_route.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/command_route_probe.py > re/out/oracle/command_route.json");
            return;
        };
        let fixture: Value = serde_json::from_slice(&bytes).unwrap();
        for (index, row) in fixture["rows"].as_array().unwrap().iter().enumerate() {
            let mut probe = Probe {
                memory: Space::new(),
                row: row.clone(),
                calls: vec![],
                depth: 0,
            };
            let base = row["memory_base"].as_u64().unwrap() as u32;
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                probe
                    .memory
                    .w32(base + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            let root = probe.name("root");
            let source = if row["case"]["source"].as_bool().unwrap_or(false) {
                probe.name("source")
            } else {
                0
            };
            let event = probe.value("event", 0x10);
            let argument = probe.value("argument", 0x87654321);
            let name = probe.value("name", 0);
            let result = dispatch(&mut probe, root, source, event, argument, name).unwrap();
            assert_eq!(result as u64, row["eax"].as_u64().unwrap(), "row{index}");
            assert_eq!(
                probe.calls,
                row["calls"].as_array().unwrap().clone(),
                "row{index} case{}",
                row["case"]
            );
            for (i, word) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    probe.memory.r32(base + i as u32 * 4) as u64,
                    word.as_u64().unwrap(),
                    "row{index} word{i}"
                );
            }
            assert_eq!(probe.depth, 0, "scratch reference leaked in row{index}");
        }
    }
}
