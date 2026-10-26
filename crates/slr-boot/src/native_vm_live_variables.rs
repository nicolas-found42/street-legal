//! Ordinary local-variable generations issued by source pool allocation,
//! construction, local append/removal, stores and destruction. Logical slots
//! bind those receipts; they never derive raw addresses from semantic indices.
use crate::native_vm_context::{Observation, ValueSnapshot};
use slr_physics::space::Space;
use slr_vm::{
    interp::VarId,
    values::{AllocOrigin, CellId, PoolEvent, PoolKind, Value},
};
use std::collections::HashMap;

#[derive(Clone)]
struct Physical {
    token: u64,
    address: u32,
    generation: u32,
    freelist: bool,
    initial: Option<ValueSnapshot>,
    retiring: Option<ValueSnapshot>,
    released: bool,
}
pub(super) enum Receipt {
    Allocate { token: u64 },
    Release { token: u64, value: ValueSnapshot },
}
struct Binding {
    token: u64,
    released: bool,
}
#[derive(Default)]
pub(super) struct Variables {
    next: u64,
    current: HashMap<u32, u64>,
    physical: HashMap<u64, Physical>,
    bound: HashMap<CellId, Binding>,
    pool: Option<u64>,
}
impl Variables {
    pub fn observe(
        &mut self,
        m: &Space,
        observation: Observation,
    ) -> Result<Option<Receipt>, String> {
        match observation {
            Observation::VariableAllocated {
                address,
                pool,
                freelist,
            } => {
                if address == 0 || pool == 0 || pool != m.r32(0x62f134) {
                    return Err("NativeVmLive: local allocation pool owner differs".into());
                }
                let contained = (0..m.r32(pool + 4)).any(|block| {
                    let base = m.r32(m.r32(pool) + block * 4);
                    address.checked_sub(base).is_some_and(|offset| {
                        offset < 0x148200
                            && offset % 16 == 0
                            && block * 0x14820 + offset / 16 < m.r32(pool + 8)
                    })
                });
                if !contained {
                    return Err(
                        "NativeVmLive: local allocation outside source variable pool".into(),
                    );
                }
                let previous = self
                    .current
                    .get(&address)
                    .and_then(|token| self.physical.get(token));
                let generation = match (freelist, previous) {
                    (false, None) => 0,
                    (true, Some(previous)) if previous.released => previous
                        .generation
                        .checked_add(1)
                        .ok_or("NativeVmLive: local source generation exhausted")?,
                    _ => {
                        return Err("NativeVmLive: local source lifetime transition differs".into())
                    }
                };
                let token = self
                    .next
                    .checked_add(1)
                    .ok_or("NativeVmLive: local token exhausted")?;
                self.next = token;
                self.current.insert(address, token);
                self.physical.insert(
                    token,
                    Physical {
                        token,
                        address,
                        generation,
                        freelist,
                        initial: None,
                        retiring: None,
                        released: false,
                    },
                );
                Ok(None)
            }
            Observation::VariableConstructed { address } => {
                let value = ValueSnapshot::read(m, address)?;
                let token = *self
                    .current
                    .get(&address)
                    .ok_or("NativeVmLive: local constructor has no allocation")?;
                let physical = self.physical.get_mut(&token).unwrap();
                if m.r32(address) != 0x5e7344
                    || physical.initial.is_some()
                    || physical.released
                    || physical.retiring.is_some()
                {
                    return Err(
                        "NativeVmLive: local construction outside source allocation lifetime"
                            .into(),
                    );
                }
                // The only named ordinary method local is the source receiver
                // variable. Its semantic name is absent; the raw canonical name
                // remains validated as its actual source constructor role.
                if !matches!(m.r32(address + 12), 0 | 0x60c37c) {
                    return Err("NativeVmLive: ordinary local constructor name role differs".into());
                }
                physical.initial = Some(value);
                Ok(Some(Receipt::Allocate { token }))
            }
            Observation::VariableReleasing { address } => {
                let value = ValueSnapshot::read(m, address)?;
                let token = *self
                    .current
                    .get(&address)
                    .ok_or("NativeVmLive: local release has no allocation")?;
                let physical = self.physical.get_mut(&token).unwrap();
                if m.r32(address) != 0x5e7344
                    || physical.initial.is_none()
                    || physical.released
                    || physical.retiring.is_some()
                {
                    return Err(
                        "NativeVmLive: local destruction outside constructed lifetime".into(),
                    );
                }
                physical.retiring = Some(value);
                Ok(None)
            }
            Observation::VariableReleased { address } => {
                let token = *self
                    .current
                    .get(&address)
                    .ok_or("NativeVmLive: local release generation absent")?;
                let physical = self.physical.get_mut(&token).unwrap();
                let value = physical
                    .retiring
                    .as_ref()
                    .ok_or("NativeVmLive: local destructor entry absent")?;
                if physical.released
                    || m.r32(m.r32(0x62f134) + 16) != address
                    || m.r32(address + 4) != value.descriptor
                    || m.r32(address + 8) != value.payload
                {
                    return Err("NativeVmLive: local source pool release differs".into());
                }
                physical.released = true;
                Ok(Some(Receipt::Release {
                    token,
                    value: value.clone(),
                }))
            }
            _ => Err("NativeVmLive: unexpected variable observation".into()),
        }
    }
    pub fn token(&self, address: u32) -> Result<u64, String> {
        let token = *self
            .current
            .get(&address)
            .ok_or("NativeVmLive: local source allocation absent")?;
        let physical = &self.physical[&token];
        if physical.released || physical.initial.is_none() {
            return Err("NativeVmLive: local source allocation is not constructed/live".into());
        }
        Ok(token)
    }
    pub fn historical(&self, id: VarId) -> Result<(u32, u64), String> {
        let id = CellId {
            pool: id.pool(),
            slot: id.slot(),
            generation: id.generation(),
        };
        let binding = self
            .bound
            .get(&id)
            .ok_or("NativeVmLive: local has no source generation binding")?;
        if binding.released {
            return Err("NativeVmLive: local semantic generation released".into());
        }
        Ok((self.physical[&binding.token].address, binding.token))
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
        if *kind != PoolKind::Variable
            || cell.kind != PoolKind::Variable
            || *slot != cell.slot
            || cell.name != Some(0)
            || cell.pool == 0
            || self.pool.is_some_and(|pool| pool != cell.pool)
        {
            return Err("NativeVmLive: ordinary local event identity/name differs".into());
        }
        let id = CellId {
            pool: cell.pool,
            slot: cell.slot,
            generation: cell.generation,
        };
        match (event, receipt) {
            (PoolEvent::Allocate { origin, .. }, Receipt::Allocate { token }) => {
                let physical = &self.physical[token];
                let initial = physical
                    .initial
                    .as_ref()
                    .ok_or("NativeVmLive: local source constructor absent")?;
                let previous = self
                    .bound
                    .iter()
                    .filter(|(id, _)| id.pool == cell.pool && id.slot == cell.slot)
                    .max_by_key(|(id, _)| id.generation);
                let logical_generation = match (origin, previous) {
                    (AllocOrigin::Pooled, None) => cell.generation == 0,
                    (AllocOrigin::Freelist, Some((id, binding))) => {
                        binding.released && id.generation.checked_add(1) == Some(cell.generation)
                    }
                    _ => false,
                };
                if !logical_generation
                    || physical.token != *token
                    || self.bound.contains_key(&id)
                    || self.bound.values().any(|binding| binding.token == *token)
                    || *origin
                        != if physical.freelist {
                            AllocOrigin::Freelist
                        } else {
                            AllocOrigin::Pooled
                        }
                    || cell.value.tag.name().as_bytes() != initial.name
                    || payload(cell.value)? != initial.payload
                {
                    return Err(
                        "NativeVmLive: local allocation differs from source constructor".into(),
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
                    .ok_or("NativeVmLive: local semantic release precedes binding")?;
                if binding.released
                    || binding.token != *token
                    || !self.physical[token].released
                    || cell.value.tag.name().as_bytes() != value.name
                    || payload(cell.value)? != value.payload
                {
                    return Err("NativeVmLive: local release differs from source destructor".into());
                }
                self.bound.get_mut(&id).unwrap().released = true;
                Ok(())
            }
            _ => Err("NativeVmLive: ordinary local pool phase differs".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slr_vm::values::PoolCellView;
    fn constructed() -> (Space, Variables, u32, u32, Receipt) {
        let mut m = Space::new();
        let pool = m.alloc(20);
        let table = m.alloc(4);
        let block = m.alloc(32);
        let descriptor = m.alloc(2);
        m.w8(descriptor, b'I');
        m.w32(0x62f134, pool);
        m.w32(pool, table);
        m.w32(pool + 4, 1);
        m.w32(pool + 8, 2);
        m.w32(pool + 12, 0x14820);
        m.w32(table, block);
        let variable = block + 16;
        let mut variables = Variables::default();
        variables
            .observe(
                &m,
                Observation::VariableAllocated {
                    address: variable,
                    pool,
                    freelist: false,
                },
            )
            .unwrap();
        m.w32(variable, 0x5e7344);
        m.w32(variable + 4, descriptor);
        m.w32(variable + 8, 0);
        m.w32(variable + 12, 0);
        let receipt = variables
            .observe(&m, Observation::VariableConstructed { address: variable })
            .unwrap()
            .unwrap();
        (m, variables, pool, variable, receipt)
    }
    fn cell(generation: u32) -> PoolCellView {
        // Source slot 1 deliberately binds semantic slot 7. Only the issued
        // generation supplies its physical pointer, never semantic arithmetic.
        PoolCellView {
            kind: PoolKind::Variable,
            pool: 19,
            slot: 7,
            generation,
            value: Value::int(0),
            name: Some(0),
        }
    }
    fn allocate(cell: PoolCellView, origin: AllocOrigin) -> PoolEvent {
        PoolEvent::Allocate {
            kind: PoolKind::Variable,
            slot: cell.slot,
            origin,
            cell,
        }
    }
    #[test]
    fn local_rejection_does_not_publish_binding_or_consume_generation() {
        let (_, mut variables, _, _, receipt) = constructed();
        let mut changed = cell(0);
        changed.value.payload = 1;
        assert!(variables
            .apply(&allocate(changed, AllocOrigin::Pooled), &receipt, |v| Ok(
                v.payload
            ))
            .is_err());
        assert!(variables.bound.is_empty());
        assert!(variables.pool.is_none());
        let changed = cell(1);
        assert!(variables
            .apply(&allocate(changed, AllocOrigin::Pooled), &receipt, |v| Ok(
                v.payload
            ))
            .is_err());
        assert!(variables.bound.is_empty());
        variables
            .apply(&allocate(cell(0), AllocOrigin::Pooled), &receipt, |v| {
                Ok(v.payload)
            })
            .unwrap();
        assert_eq!(variables.bound.len(), 1);
        assert!(variables
            .apply(&allocate(cell(0), AllocOrigin::Pooled), &receipt, |v| Ok(
                v.payload
            ))
            .is_err());
        assert_eq!(variables.bound.len(), 1);
    }
    #[test]
    fn local_pool_reuse_keeps_distinct_source_and_semantic_generations() {
        let (mut m, mut variables, pool, address, receipt) = constructed();
        variables
            .apply(&allocate(cell(0), AllocOrigin::Pooled), &receipt, |v| {
                Ok(v.payload)
            })
            .unwrap();
        let first = variables.current[&address];
        variables
            .observe(&m, Observation::VariableReleasing { address })
            .unwrap();
        m.w32(address, 0);
        m.w32(pool + 16, address);
        let released = variables
            .observe(&m, Observation::VariableReleased { address })
            .unwrap()
            .unwrap();
        let event = PoolEvent::Release {
            kind: PoolKind::Variable,
            slot: 7,
            cell: cell(0),
        };
        let mut forged = cell(0);
        forged.generation = 1;
        assert!(variables
            .apply(
                &PoolEvent::Release {
                    kind: PoolKind::Variable,
                    slot: 7,
                    cell: forged
                },
                &released,
                |v| Ok(v.payload)
            )
            .is_err());
        assert!(
            !variables.bound[&CellId {
                pool: 19,
                slot: 7,
                generation: 0
            }]
                .released
        );
        variables
            .apply(&event, &released, |v| Ok(v.payload))
            .unwrap();
        variables
            .observe(
                &m,
                Observation::VariableAllocated {
                    address,
                    pool,
                    freelist: true,
                },
            )
            .unwrap();
        m.w32(address, 0x5e7344);
        let next = variables
            .observe(&m, Observation::VariableConstructed { address })
            .unwrap()
            .unwrap();
        let second = variables.current[&address];
        assert_ne!(first, second);
        assert_eq!(variables.physical[&second].generation, 1);
        assert!(variables
            .apply(&allocate(cell(0), AllocOrigin::Freelist), &next, |v| Ok(
                v.payload
            ))
            .is_err());
        variables
            .apply(&allocate(cell(1), AllocOrigin::Freelist), &next, |v| {
                Ok(v.payload)
            })
            .unwrap();
        assert_eq!(variables.bound.len(), 2);
        assert!(
            variables.bound[&CellId {
                pool: 19,
                slot: 7,
                generation: 0
            }]
                .released
        );
        assert!(
            !variables.bound[&CellId {
                pool: 19,
                slot: 7,
                generation: 1
            }]
                .released
        );
    }
}
