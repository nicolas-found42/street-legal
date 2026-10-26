//! Global value-cell lifetimes issued by actual allocator and constructor
//! bodies. Logical cell identities bind to those receipts, never to addresses
//! calculated from their slot numbers.
use crate::native_vm_values::Observation;
use slr_physics::space::Space;
use slr_vm::values::{AllocOrigin, CellId, PoolEvent, PoolKind, Value};
use std::collections::HashMap;
#[derive(Clone)]
pub(super) struct RawValue {
    descriptor: u32,
    name: Vec<u8>,
    payload: u32,
}
struct Physical {
    address: u32,
    token: u64,
    slot: u32,
    generation: u32,
    freelist: bool,
    initial: Option<RawValue>,
    retiring: Option<RawValue>,
    released: bool,
}
pub(super) enum Receipt {
    Allocate { token: u64 },
    Release { token: u64, value: RawValue },
}
struct Binding {
    token: u64,
    released: bool,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct ValueHistory {
    pub token: u64,
    pub address: u32,
    pub slot: u32,
    pub generation: u32,
    pub freelist: bool,
    pub initial: Option<(u32, Vec<u8>, u32)>,
    pub released: bool,
    pub bindings: Vec<CellId>,
}
#[derive(Default)]
pub(super) struct Values {
    next: u64,
    current: HashMap<u32, u64>,
    history: HashMap<u64, Physical>,
    bound: HashMap<CellId, Binding>,
    pool: Option<u64>,
}
fn value(m: &Space, address: u32) -> Result<RawValue, String> {
    if m.r32(address) != 0x5e734c {
        return Err("NativeVmLive: value constructor/destructor virtual type differs".into());
    }
    let descriptor = m.r32(address + 4);
    if descriptor == 0 {
        return Err("NativeVmLive: value descriptor absent".into());
    }
    let mut name = Vec::new();
    for i in 0..0x10000 {
        let byte = m.r8(descriptor + i);
        if byte == 0 {
            return Ok(RawValue {
                descriptor,
                name,
                payload: m.r32(address + 8),
            });
        }
        name.push(byte);
    }
    Err("NativeVmLive: value descriptor unterminated".into())
}
impl Values {
    pub fn history(&self) -> Vec<ValueHistory> {
        let mut history = self
            .history
            .values()
            .map(|physical| ValueHistory {
                token: physical.token,
                address: physical.address,
                slot: physical.slot,
                generation: physical.generation,
                freelist: physical.freelist,
                initial: physical
                    .initial
                    .as_ref()
                    .map(|value| (value.descriptor, value.name.clone(), value.payload)),
                released: physical.released,
                bindings: self
                    .bound
                    .iter()
                    .filter_map(|(id, binding)| (binding.token == physical.token).then_some(*id))
                    .collect(),
            })
            .collect::<Vec<_>>();
        history.sort_by_key(|value| value.token);
        history
    }
    pub fn observe(
        &mut self,
        m: &Space,
        observation: Observation,
    ) -> Result<Option<Receipt>, String> {
        match observation {
            Observation::Allocated {
                address,
                pool,
                freelist,
            } => {
                if pool == 0 || pool != m.r32(0x62f130) || address == 0 {
                    return Err("NativeVmLive: value allocation pool owner differs".into());
                }
                let mut slot = None;
                for block in 0..m.r32(pool + 4) {
                    let base = m.r32(m.r32(pool) + block * 4);
                    if let Some(offset) = address
                        .checked_sub(base)
                        .filter(|offset| *offset < 0x1800 && offset % 12 == 0)
                    {
                        let index = block * 512 + offset / 12;
                        if index < m.r32(pool + 8) {
                            slot = Some(index);
                            break;
                        }
                    }
                }
                let slot = slot.ok_or(
                    "NativeVmLive: value allocation is outside source-issued pool storage",
                )?;
                let previous = self
                    .current
                    .get(&address)
                    .and_then(|token| self.history.get(token));
                let generation = match (freelist, previous) {
                    (false, None) => 0,
                    (true, Some(previous)) if previous.released => previous
                        .generation
                        .checked_add(1)
                        .ok_or("NativeVmLive: value generation exhausted")?,
                    _ => return Err(
                        "NativeVmLive: value allocation has no matching source lifetime transition"
                            .into(),
                    ),
                };
                let token = self
                    .next
                    .checked_add(1)
                    .ok_or("NativeVmLive: value token exhausted")?;
                self.next = token;
                self.current.insert(address, token);
                self.history.insert(
                    token,
                    Physical {
                        address,
                        token,
                        slot,
                        generation,
                        freelist,
                        initial: None,
                        retiring: None,
                        released: false,
                    },
                );
                Ok(None)
            }
            Observation::Constructed { address } => {
                let raw = value(m, address)?;
                let token = *self
                    .current
                    .get(&address)
                    .ok_or("NativeVmLive: value construction has no source allocation")?;
                let physical = self.history.get_mut(&token).unwrap();
                if physical.released || physical.retiring.is_some() || physical.initial.is_some() {
                    return Err(
                        "NativeVmLive: value constructed outside its allocation lifetime".into(),
                    );
                }
                physical.initial = Some(raw);
                Ok(Some(Receipt::Allocate { token }))
            }
            Observation::Releasing { address } => {
                let raw = value(m, address)?;
                let token = self
                    .current
                    .get(&address)
                    .ok_or("NativeVmLive: value release has no source allocation")?;
                let physical = self.history.get_mut(token).unwrap();
                if physical.released || physical.retiring.is_some() {
                    return Err("NativeVmLive: repeated source value release".into());
                }
                physical.retiring = Some(raw);
                Ok(None)
            }
            Observation::Released { address } => {
                let token = *self
                    .current
                    .get(&address)
                    .ok_or("NativeVmLive: released value has no source allocation")?;
                let physical = self.history.get_mut(&token).unwrap();
                let raw = physical
                    .retiring
                    .as_ref()
                    .ok_or("NativeVmLive: released value has no destructor entry")?;
                if physical.released
                    || m.r32(m.r32(0x62f130) + 16) != address
                    || m.r32(address + 4) != raw.descriptor
                    || m.r32(address + 8) != raw.payload
                {
                    return Err(
                        "NativeVmLive: released value differs from actual pool handoff".into(),
                    );
                }
                physical.released = true;
                Ok(Some(Receipt::Release {
                    token,
                    value: raw.clone(),
                }))
            }
        }
    }
    pub fn token(&self, address: u32) -> Result<u64, String> {
        let token = *self
            .current
            .get(&address)
            .ok_or("NativeVmLive: operand value has no source allocation")?;
        if self.history[&token].released {
            return Err("NativeVmLive: operand value was already released".into());
        }
        Ok(token)
    }
    pub fn historical(&self, id: CellId) -> Result<(u32, u64), String> {
        let binding = self
            .bound
            .get(&id)
            .ok_or("NativeVmLive: cell has no issued source binding")?;
        if binding.released {
            return Err("NativeVmLive: logical value lifetime already released".into());
        }
        Ok((self.history[&binding.token].address, binding.token))
    }
    pub fn live(&self, m: &Space, id: CellId) -> Result<u32, String> {
        let (address, token) = self.historical(id)?;
        let physical = &self.history[&token];
        if self.current.get(&address) != Some(&token)
            || physical.released
            || physical.retiring.is_some()
            || m.r32(address) != 0x5e734c
        {
            return Err("NativeVmLive: historical cell is not a live source value".into());
        }
        Ok(address)
    }
    pub fn apply(
        &mut self,
        event: &PoolEvent,
        receipt: &Receipt,
        mut payload: impl FnMut(Value) -> Result<u32, String>,
    ) -> Result<(), String> {
        let (kind, slot, cell) = match event {
            PoolEvent::Allocate {
                kind, slot, cell, ..
            }
            | PoolEvent::Release { kind, slot, cell } => (kind, slot, cell),
        };
        if *kind != PoolKind::Value
            || cell.kind != PoolKind::Value
            || *slot != cell.slot
            || cell.name.is_some()
            || cell.pool == 0
            || self.pool.is_some_and(|pool| pool != cell.pool)
        {
            return Err("NativeVmLive: value event pool/identity differs".into());
        }
        let id = CellId {
            pool: cell.pool,
            slot: cell.slot,
            generation: cell.generation,
        };
        match (event, receipt) {
            (PoolEvent::Allocate { origin, .. }, Receipt::Allocate { token }) => {
                let physical = &self.history[token];
                let initial = physical
                    .initial
                    .as_ref()
                    .ok_or("NativeVmLive: source value constructor role absent")?;
                if physical.token != *token
                    || cell.slot != physical.slot
                    || cell.generation != physical.generation
                    || *origin
                        != if physical.freelist {
                            AllocOrigin::Freelist
                        } else {
                            AllocOrigin::Pooled
                        }
                    || self.bound.contains_key(&id)
                    || cell.value.tag.name().as_bytes() != initial.name
                    || payload(cell.value)? != initial.payload
                {
                    return Err(
                        "NativeVmLive: value allocation event differs from source constructor"
                            .into(),
                    );
                }
                self.pool = Some(cell.pool);
                self.bound.insert(
                    id,
                    Binding {
                        token: *token,
                        released: false,
                    },
                );
                Ok(())
            }
            (PoolEvent::Release { .. }, Receipt::Release { token, value }) => {
                let binding = self
                    .bound
                    .get(&id)
                    .ok_or("NativeVmLive: value released before its issued binding")?;
                if binding.released
                    || binding.token != *token
                    || !self.history[token].released
                    || cell.value.tag.name().as_bytes() != value.name
                    || payload(cell.value)? != value.payload
                {
                    return Err(
                        "NativeVmLive: value release event differs from source destructor".into(),
                    );
                }
                self.bound.get_mut(&id).unwrap().released = true;
                Ok(())
            }
            _ => Err("NativeVmLive: value pool event differs from next source phase".into()),
        }
    }
}
