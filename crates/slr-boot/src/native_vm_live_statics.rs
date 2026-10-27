//! Static variables issued by403e80 after their actual owner append. Semantic
//! slots select those receipts; they cannot allocate or initialize a cell.
use super::Metadata;
use slr_physics::space::Space;
use slr_vm::{interp::HeapSlot, machine::Event};
use std::collections::HashMap;
#[derive(Clone)]
pub(super) struct Receipt {
    token: u64,
    class: u32,
    index: usize,
    address: u32,
    owner: u32,
    record: u32,
    descriptor: u32,
    descriptor_bytes: Vec<u8>,
    name: u32,
}
#[derive(Default)]
pub(super) struct Statics {
    next: u64,
    physical: HashMap<u32, Vec<Receipt>>,
    variables: HashMap<u32, u64>,
    bound: HashMap<(u32, usize), Receipt>,
}
fn bytes(m: &Space, pointer: u32) -> Result<Vec<u8>, String> {
    if pointer == 0 {
        return Err("NativeVmLive: static descriptor/name absent".into());
    }
    let mut bytes = Vec::new();
    for i in 0..0x10000 {
        let byte = m.r8(pointer + i);
        if byte == 0 {
            return Ok(bytes);
        }
        bytes.push(byte);
    }
    Err("NativeVmLive: unterminated static descriptor/name".into())
}
impl Statics {
    pub fn observe(
        &mut self,
        m: &Space,
        jvm: u32,
        class: u32,
        index: usize,
        variable: u32,
        record: u32,
    ) -> Result<Receipt, String> {
        let owner = m.r32(class + 0x1d0);
        let vector = m.r32(owner + 12);
        let fields = m.r32(m.r32(class + 4) + 0x54);
        if class == 0
            || m.r32(class) != 0x5e7340
            || m.r32(class + 12) != jvm
            || owner == 0
            || m.r32(owner) != 0x5e7338
            || m.r32(owner + 16) != jvm
            || index != self.physical.get(&class).map_or(0, Vec::len)
            || m.r32(vector + 8) as usize != index + 1
            || m.r32(m.r32(vector + 4) + index as u32 * 4) != variable
            || variable == 0
            || self.variables.contains_key(&variable)
            || m.r32(variable) != 0x5e7344
            || m.r32(variable + 8) != 0
            || index >= m.r32(fields + 12) as usize
            || m.r32(m.r32(fields + 8) + index as u32 * 4) != record
            || m.r32(record) != 0x5f0890
        {
            return Err("NativeVmLive: static variable differs from actual source append".into());
        }
        let descriptor = m.r32(variable + 4);
        let descriptor_bytes = bytes(m, descriptor)?;
        let name = m.r32(variable + 12);
        bytes(m, name)?;
        let token = self
            .next
            .checked_add(1)
            .ok_or("NativeVmLive: static generation exhausted")?;
        let receipt = Receipt {
            token,
            class,
            index,
            address: variable,
            owner,
            record,
            descriptor,
            descriptor_bytes,
            name,
        };
        self.next = token;
        self.variables.insert(variable, token);
        self.physical
            .entry(class)
            .or_default()
            .push(receipt.clone());
        Ok(receipt)
    }
    pub fn apply(
        &mut self,
        m: &Space,
        event: &Event,
        metadata: &Metadata,
        receipt: &Receipt,
    ) -> Result<(), String> {
        let Event::HeapConstruct {
            slot: HeapSlot::Static { context, slot },
            value,
        } = event
        else {
            return Err("NativeVmLive: expected next source-issued static construction".into());
        };
        if metadata.class(*context)?.class != receipt.class
            || *slot != receipt.index
            || metadata.field(m, *context, true, *slot)? != receipt.record
            || value.payload != 0
            || value.tag.name().as_bytes() != receipt.descriptor_bytes
            || self.bound.contains_key(&(*context, *slot))
        {
            return Err(
                "NativeVmLive: static construction event differs from its source declaration"
                    .into(),
            );
        }
        self.validate(m, receipt)?;
        self.bound.insert((*context, *slot), receipt.clone());
        Ok(())
    }
    fn validate(&self, m: &Space, receipt: &Receipt) -> Result<(), String> {
        let vector = m.r32(receipt.owner + 12);
        if self.variables.get(&receipt.address) != Some(&receipt.token)
            || m.r32(receipt.class + 0x1d0) != receipt.owner
            || m.r32(receipt.owner) != 0x5e7338
            || m.r32(vector + 8) as usize <= receipt.index
            || m.r32(m.r32(vector + 4) + receipt.index as u32 * 4) != receipt.address
            || m.r32(receipt.address) != 0x5e7344
            || m.r32(receipt.address + 4) != receipt.descriptor
            || m.r32(receipt.address + 12) != receipt.name
        {
            return Err("NativeVmLive: static source variable generation/owner changed".into());
        }
        Ok(())
    }
    pub fn field(
        &self,
        m: &Space,
        context: u32,
        index: usize,
        metadata: &Metadata,
    ) -> Result<u32, String> {
        let receipt = self
            .bound
            .get(&(context, index))
            .ok_or("NativeVmLive: static slot has no issued raw binding")?;
        if metadata.class(context)?.class != receipt.class
            || metadata.field(m, context, true, index)? != receipt.record
        {
            return Err("NativeVmLive: static declaring record changed".into());
        }
        self.validate(m, receipt)?;
        Ok(receipt.address)
    }
}
