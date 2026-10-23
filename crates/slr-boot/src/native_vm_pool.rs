//! Raw pool memory projection from explicit VM allocation lifetimes.
//!
//! This bounded seam covers allocator/header/cell bytes. It does not execute
//! Thread.run, materialize interpreter scratch, or establish caller registers.
//! The callback ABI guard stays in place. Empty original pool headers and the
//! actual allocator context are required; a pre-existing history is rejected.
use slr_physics::space::Space;
use slr_vm::values::{AllocOrigin, PoolCellView, PoolEvent, PoolKind};
use std::collections::HashMap;

const LIVE: u32 = 0x62f138;
const VALUE_VTABLE: u32 = 0x5e734c;
const VAR_VTABLE: u32 = 0x5e7344;

/// Addresses must come from an explicit descriptor/reference/name resolver.
/// Semantic IDs and matching payloads never establish an original address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawWords {
    pub descriptor: u32,
    pub payload: u32,
    pub name: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as Json;
    use sha2::{Digest, Sha256};
    use slr_vm::values::Value;

    pub(super) struct Bump;
    impl Allocator for Bump {
        fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String> {
            Ok(m.alloc(bytes.max(1)))
        }
        fn reallocate(
            &mut self,
            m: &mut Space,
            old: u32,
            old_bytes: u32,
            bytes: u32,
        ) -> Result<u32, String> {
            let p = m.alloc(bytes.max(1));
            for i in 0..old_bytes.min(bytes) {
                m.w8(p + i, m.r8(old + i));
            }
            Ok(p)
        }
    }
    struct Words(u32);
    impl Resolver for Words {
        fn resolve(&mut self, cell: PoolCellView) -> Result<RawWords, String> {
            Ok(RawWords {
                descriptor: self.0,
                payload: cell.value.payload,
                name: cell.name,
            })
        }
    }
    fn word(v: &Json) -> u32 {
        u32::try_from(v.as_u64().unwrap()).unwrap()
    }
    fn bytes(path: &std::path::Path, field: &str, v: &Json) -> Vec<u8> {
        let result = std::process::Command::new("python3")
            .args(["-c", "import base64,json,sys,zlib; d=json.load(open(sys.argv[1]))[sys.argv[2]]; sys.stdout.buffer.write(zlib.decompress(base64.b64decode(d['zlib_base64'])))"])
            .arg(path).arg(field).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout.len(), word(&v[field]["length"]) as usize);
        assert_eq!(
            format!("{:x}", Sha256::digest(&result.stdout)),
            v[field]["sha256"]
        );
        result.stdout
    }
    #[test]
    fn original_pool_headers_cells_heap_and_ordered_stores_match() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("re/out/boot/native-vm-pool-original.json");
        let v: Json =
            serde_json::from_slice(&std::fs::read(&path).expect("run native_vm_pool_probe.py"))
                .unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["profile"]["fpu_cw"], 0x7f);
        for (field, file) in [
            ("producer_sha256", "re/oracle/native_vm_pool_probe.py"),
            ("oracle_sha256", "re/oracle/oracle.py"),
            ("scenario_sha256", "re/oracle/thread_vectors.py"),
        ] {
            assert_eq!(
                v[field],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(file)).unwrap())
                )
            );
        }
        assert_eq!(
            v["profile"]["original_exe_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
                )
            )
        );
        let mut m = Space::new();
        for (i, b) in bytes(&path, "initial", &v).into_iter().enumerate() {
            m.w8(0x20000000 + i as u32, b);
        }
        m.set_heap_ptr(word(&v["initial"]["heap_ptr"]));
        m.w32(0x62f130, word(&v["pools"]["value"]));
        m.w32(0x62f134, word(&v["pools"]["variable"]));
        m.w32(LIVE, word(&v["initial"]["live"]));
        let mut pools = Pools::from_empty(&m).unwrap();
        let mut resolver = Words(word(&v["type_pointer"]));
        let mut cells = HashMap::<(PoolKind, u32), PoolCellView>::new();
        let mut generations = HashMap::<(PoolKind, u32), u32>::new();
        let mut free = [Vec::<u32>::new(), Vec::<u32>::new()];
        STORES.with(|s| s.borrow_mut().clear());
        let ops = v["ops"].as_array().unwrap();
        assert_eq!(ops.len(), 526, "complete growth/reuse sequence");
        for op in ops {
            let kind = if op["kind"] == "value" {
                PoolKind::Value
            } else {
                assert_eq!(op["kind"], "variable");
                PoolKind::Variable
            };
            let slot = word(&op["slot"]);
            let key = (kind, slot);
            let event = if op["action"] == "allocate" {
                let origin = if let Some(next) = free[index(kind)].pop() {
                    assert_eq!(next, slot);
                    AllocOrigin::Freelist
                } else {
                    AllocOrigin::Pooled
                };
                let cell = PoolCellView {
                    kind,
                    pool: 1 + index(kind) as u64,
                    slot,
                    generation: *generations.entry(key).or_default(),
                    value: Value::int(word(&op["payload"]) as i32),
                    name: (kind == PoolKind::Variable).then(|| word(&op["name"])),
                };
                if kind == PoolKind::Value {
                    m.w32(word(&v["source"]) + 8, cell.value.payload);
                }
                cells.insert(key, cell);
                PoolEvent::Allocate {
                    kind,
                    slot,
                    origin,
                    cell,
                }
            } else {
                assert_eq!(op["action"], "release");
                let cell = cells.remove(&key).unwrap();
                free[index(kind)].push(slot);
                *generations.entry(key).or_default() += 1;
                PoolEvent::Release { kind, slot, cell }
            };
            assert_eq!(
                pools
                    .apply(&mut m, &event, &mut resolver, &mut Bump)
                    .unwrap(),
                word(&op["address"])
            );
        }
        let expected = bytes(&path, "final", &v);
        assert_eq!(m.heap_ptr(), word(&v["final"]["heap_ptr"]));
        assert_eq!(
            (0..expected.len())
                .map(|i| m.r8(0x20000000 + i as u32))
                .collect::<Vec<_>>(),
            expected,
            "all heap bytes including freed cells and realloc retention"
        );
        assert_eq!(m.r32(LIVE), word(&v["final"]["live"]));
        let stores = v["stores"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                assert_eq!(s["size"], 4);
                (word(&s["address"]), word(&s["value"]))
            })
            .collect::<Vec<_>>();
        STORES.with(|s| {
            assert_eq!(
                *s.borrow(),
                stores,
                "every ordered original pool/cell/live store"
            )
        });
    }
    #[test]
    fn rejects_preexisting_history_and_unresolved_identity_without_writes() {
        let mut m = Space::new();
        m.w32(0x62f130, 0x20000000);
        m.w32(0x62f134, 0x20000020);
        m.w32(0x20000008, 1);
        assert!(Pools::from_empty(&m).is_err());
        m.w32(0x20000008, 0);
        let mut pools = Pools::from_empty(&m).unwrap();
        let cell = PoolCellView {
            kind: PoolKind::Value,
            pool: 7,
            slot: 1,
            generation: 0,
            value: Value::int(42),
            name: None,
        };
        STORES.with(|s| s.borrow_mut().clear());
        assert!(pools
            .apply(
                &mut m,
                &PoolEvent::Allocate {
                    kind: cell.kind,
                    slot: cell.slot,
                    origin: AllocOrigin::Pooled,
                    cell
                },
                &mut Words(0x60ab50),
                &mut Bump
            )
            .is_err());
        assert!(pools.address(cell).is_err());
        STORES.with(|s| assert!(s.borrow().is_empty()));
    }
}
pub trait Resolver {
    fn resolve(&mut self, cell: PoolCellView) -> Result<RawWords, String>;
}
/// Bind a descriptor/name to one explicit allocation lifetime. In particular,
/// equal type spellings or value payloads cannot select an existing raw cell.
/// Source hooks supply descriptor/name words from their actual caller metadata;
/// source404e20 results separately bind semantic instance IDs to raw instances.
#[derive(Default)]
pub struct OwnerResolver {
    cells: HashMap<(PoolKind, u64, u32, u32), (u32, Option<u32>)>,
    instances: HashMap<u32, u32>,
}
#[allow(dead_code)]
impl OwnerResolver {
    fn key(cell: PoolCellView) -> (PoolKind, u64, u32, u32) {
        (cell.kind, cell.pool, cell.slot, cell.generation)
    }
    pub fn bind_cell(
        &mut self,
        cell: PoolCellView,
        descriptor: u32,
        name: Option<u32>,
    ) -> Result<(), String> {
        if descriptor == 0
            || name.is_some() != (cell.kind == PoolKind::Variable)
            || cell.name.is_some() != name.is_some()
        {
            return Err("NativeVmPool: unresolved source cell descriptor/name".into());
        }
        let key = Self::key(cell);
        let raw = (descriptor, name);
        if self
            .cells
            .get(&key)
            .is_some_and(|previous| *previous != raw)
        {
            return Err("NativeVmPool: contradictory allocation-lifetime metadata".into());
        }
        self.cells.insert(key, raw);
        Ok(())
    }
    pub fn forget_cell(&mut self, cell: PoolCellView) {
        self.cells.remove(&Self::key(cell));
    }
    /// Called only at actual source instance construction, before any Ref cell
    /// exposes that instance. Recycled semantic IDs require explicit removal.
    pub fn bind_instance(&mut self, semantic: u32, raw: u32) -> Result<(), String> {
        if semantic == 0 || raw == 0 || self.instances.get(&semantic).is_some_and(|p| *p != raw) {
            return Err("NativeVmPool: unresolved/contradictory live instance identity".into());
        }
        self.instances.insert(semantic, raw);
        Ok(())
    }
    pub fn forget_instance(&mut self, semantic: u32) {
        self.instances.remove(&semantic);
    }
}
impl Resolver for OwnerResolver {
    fn resolve(&mut self, cell: PoolCellView) -> Result<RawWords, String> {
        let (descriptor, name) = *self
            .cells
            .get(&Self::key(cell))
            .ok_or("NativeVmPool: missing source metadata for exact cell owner")?;
        let payload = if cell.value.tag.is_ref() && cell.value.payload != 0 {
            *self
                .instances
                .get(&cell.value.payload)
                .ok_or("NativeVmPool: missing source instance identity")?
        } else {
            cell.value.payload
        };
        Ok(RawWords {
            descriptor,
            payload,
            name,
        })
    }
}
/// These collaborators carry the real allocator chronology. The bounded
/// differential profile supplies zeroed bump malloc and allocate/copy realloc.
pub trait Allocator {
    fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String>;
    fn reallocate(
        &mut self,
        m: &mut Space,
        old: u32,
        old_bytes: u32,
        bytes: u32,
    ) -> Result<u32, String>;
}
#[derive(Clone, Copy, Debug)]
struct Binding {
    generation: u32,
    live: bool,
    address: u32,
}
#[derive(Debug)]
pub struct Pools {
    headers: [u32; 2],
    arenas: [Option<u64>; 2],
    bindings: HashMap<(PoolKind, u32), Binding>,
}
fn index(kind: PoolKind) -> usize {
    if kind == PoolKind::Value {
        0
    } else {
        1
    }
}
fn geometry(kind: PoolKind) -> (u32, u32) {
    if kind == PoolKind::Value {
        (512, 12)
    } else {
        (84000, 16)
    }
}
#[cfg(test)]
thread_local! { static STORES: std::cell::RefCell<Vec<(u32,u32)>> = const { std::cell::RefCell::new(Vec::new()) }; }
fn write(m: &mut Space, address: u32, word: u32) {
    #[cfg(test)]
    STORES.with(|s| s.borrow_mut().push((address, word)));
    m.w32(address, word);
}
// Pending root: this memory seam must not bypass missing Thread.run machine
// metadata, opcode scratch and callback caller-history requirements.
#[allow(dead_code)]
impl Pools {
    pub fn from_empty(m: &Space) -> Result<Self, String> {
        let headers = [m.r32(0x62f130), m.r32(0x62f134)];
        if headers.contains(&0)
            || headers[0] == headers[1]
            || headers
                .iter()
                .any(|h| (0..5).any(|i| m.r32(h + 4 * i) != 0))
        {
            return Err("NativeVmPool: empty distinct original headers required; history cannot be reconstructed".into());
        }
        Ok(Self {
            headers,
            arenas: [None, None],
            bindings: HashMap::new(),
        })
    }
    fn validate(&self, cell: PoolCellView, raw: RawWords) -> Result<usize, String> {
        let i = index(cell.kind);
        if self.arenas[i].is_some_and(|id| id != cell.pool)
            || (cell.kind == PoolKind::Variable) != raw.name.is_some()
            || (cell.kind == PoolKind::Variable) != cell.name.is_some()
            || raw.descriptor == 0
        {
            return Err("NativeVmPool: unresolved descriptor/name or foreign arena".into());
        }
        Ok(i)
    }
    pub fn address(&self, cell: PoolCellView) -> Result<u32, String> {
        if self.arenas[index(cell.kind)] != Some(cell.pool) {
            return Err("NativeVmPool: unknown arena".into());
        }
        let binding = self
            .bindings
            .get(&(cell.kind, cell.slot))
            .filter(|b| b.live && b.generation == cell.generation)
            .ok_or("NativeVmPool: missing/stale/non-owned cell")?;
        Ok(binding.address)
    }
    /// Apply allocation/free in observed order. All fallible identity and
    /// resolver checks precede memory writes. Allocator failure can retain the
    /// same partial growth writes as the original and is not rolled back.
    pub fn apply(
        &mut self,
        m: &mut Space,
        event: &PoolEvent,
        resolver: &mut impl Resolver,
        allocator: &mut impl Allocator,
    ) -> Result<u32, String> {
        let (kind, slot, cell) = match *event {
            PoolEvent::Allocate {
                kind, slot, cell, ..
            }
            | PoolEvent::Release { kind, slot, cell } => (kind, slot, cell),
        };
        if kind != cell.kind || slot != cell.slot {
            return Err("NativeVmPool: inconsistent event identity".into());
        }
        let raw = resolver.resolve(cell)?;
        let i = self.validate(cell, raw)?;
        let h = self.headers[i];
        match *event {
            PoolEvent::Allocate { origin, .. } => {
                let (count, stride) = geometry(kind);
                let free = m.r32(h + 16);
                let binding = self.bindings.get(&(kind, slot)).copied();
                match origin {
                    AllocOrigin::Freelist
                        if free != 0
                            && binding.is_some_and(|b| {
                                !b.live && b.address == free && b.generation == cell.generation
                            }) => {}
                    AllocOrigin::Pooled
                        if free == 0
                            && slot == m.r32(h + 8)
                            && binding.is_none()
                            && cell.generation == 0 => {}
                    _ => return Err("NativeVmPool: chronology/generation/freelist mismatch".into()),
                }
                let pointer = if free != 0 {
                    write(m, h + 16, m.r32(free));
                    free
                } else {
                    let handed = m.r32(h + 8);
                    if handed == m.r32(h + 12) {
                        let blocks = m.r32(h + 4);
                        write(m, h + 12, m.r32(h + 12) + count);
                        write(m, h + 4, blocks + 1);
                        let table =
                            allocator.reallocate(m, m.r32(h), blocks * 4, (blocks + 1) * 4)?;
                        write(m, h, table);
                        let block = allocator.allocate(m, count * stride)?;
                        write(m, table + blocks * 4, block);
                    }
                    write(m, h + 8, handed + 1);
                    m.r32(m.r32(h) + (handed / count) * 4) + (handed % count) * stride
                };
                //423e30 copy construction;423ec0 variable construction uses
                //the typed zero-value path then its name/vtable suffix.
                write(m, pointer, VALUE_VTABLE);
                write(m, pointer + 4, raw.descriptor);
                write(
                    m,
                    pointer + 8,
                    if kind == PoolKind::Variable {
                        0
                    } else {
                        raw.payload
                    },
                );
                write(m, LIVE, m.r32(LIVE).wrapping_add(1));
                if let Some(name) = raw.name {
                    write(m, pointer, VAR_VTABLE);
                    write(m, pointer + 12, name);
                    // A nonzero semantic initial variable requires a later
                    // explicit store, outside this constructor projection.
                }
                self.arenas[i] = Some(cell.pool);
                self.bindings.insert(
                    (kind, slot),
                    Binding {
                        generation: cell.generation,
                        live: true,
                        address: pointer,
                    },
                );
                Ok(pointer)
            }
            PoolEvent::Release { .. } => {
                let pointer = self.address(cell)?;
                if m.r32(pointer + 4) != raw.descriptor
                    || m.r32(pointer + 8) != raw.payload
                    || raw.name.is_some_and(|n| m.r32(pointer + 12) != n)
                {
                    return Err("NativeVmPool: changed cell requires explicit synchronization before release".into());
                }
                if kind == PoolKind::Variable {
                    write(m, pointer, VAR_VTABLE);
                }
                write(m, pointer, VALUE_VTABLE);
                write(m, LIVE, m.r32(LIVE).wrapping_sub(1));
                write(m, pointer, m.r32(h + 16));
                write(m, h + 16, pointer);
                let b = self.bindings.get_mut(&(kind, slot)).unwrap();
                b.live = false;
                b.generation = b.generation.wrapping_add(1);
                Ok(pointer)
            }
        }
    }
    /// Explicit boundary synchronization of an identified live owner. This
    /// provides current cell bytes, not the missing opcode store chronology.
    pub fn synchronize(
        &self,
        m: &mut Space,
        cell: PoolCellView,
        resolver: &mut impl Resolver,
    ) -> Result<u32, String> {
        let raw = resolver.resolve(cell)?;
        self.validate(cell, raw)?;
        let pointer = self.address(cell)?;
        write(m, pointer + 4, raw.descriptor);
        write(m, pointer + 8, raw.payload);
        if let Some(name) = raw.name {
            write(m, pointer + 12, name);
        }
        Ok(pointer)
    }
}
