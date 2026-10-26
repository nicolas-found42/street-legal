//! Source-issued ordinary objects and their owned instance variables. A field
//! is selected by its issued owner and ordinal, never by a matching payload.
use super::Metadata;
use crate::native_vm_context::Observation;
use slr_physics::space::Space;
use slr_vm::{interp::HeapSlot, machine::Event};
use std::collections::HashMap;
#[derive(Clone)]
pub(super) struct Field {
    token: u64,
    address: u32,
    object: u64,
    index: usize,
    descriptor: u32,
    name: u32,
    descriptor_bytes: Vec<u8>,
    declaring: u32,
    declared_index: usize,
    record: u32,
}
#[derive(Clone)]
struct Array {
    descriptor: u32,
    descriptor_bytes: Vec<u8>,
    length: u32,
    storage: u32,
    hidden: u32,
    elements: Vec<(u32, u64)>,
}
struct Object {
    token: u64,
    address: u32,
    class: u32,
    fields: Vec<Field>,
    array: Option<Array>,
    committed: bool,
}
pub(super) enum Receipt {
    Object {
        token: u64,
        address: u32,
        class: u32,
    },
    Field(Field),
}
struct Binding {
    token: u64,
    address: u32,
    class: u32,
    fields: Vec<Field>,
}
#[derive(Default)]
pub(super) struct Objects {
    next: u64,
    physical: HashMap<u32, Object>,
    variables: HashMap<u32, u64>,
    bound: HashMap<u32, Binding>,
    semantic: HashMap<u32, u32>,
}
fn bytes(m: &Space, pointer: u32) -> Result<Vec<u8>, String> {
    if pointer == 0 {
        return Err("NativeVmLive: issued descriptor/name is absent".into());
    }
    let mut bytes = Vec::new();
    for i in 0..0x10000 {
        let byte = m.r8(pointer + i);
        if byte == 0 {
            return Ok(bytes);
        }
        bytes.push(byte);
    }
    Err("NativeVmLive: issued descriptor/name is unterminated".into())
}
impl Objects {
    fn token(&mut self) -> Result<u64, String> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or("NativeVmLive: object/field generation exhausted")?;
        Ok(self.next)
    }
    pub fn observe(
        &mut self,
        m: &Space,
        jvm: u32,
        observation: Observation,
    ) -> Result<Option<Receipt>, String> {
        match observation {
            Observation::ObjectBegin {
                address,
                class,
                length,
            } => {
                let descriptor = m.r32(m.r32(class + 8));
                let descriptor_bytes = bytes(m, descriptor)?;
                let array = if descriptor_bytes.first() == Some(&b'[') {
                    Some(Array {
                        descriptor,
                        descriptor_bytes,
                        length,
                        storage: 0,
                        hidden: 0,
                        elements: Vec::new(),
                    })
                } else {
                    if length != 0 {
                        return Err("NativeVmLive: ordinary object received array length".into());
                    }
                    None
                };
                if address == 0
                    || self.physical.contains_key(&address)
                    || m.r32(address) != 0x5e7354
                    || m.r32(address + 12) != class
                    || m.r32(class) != 0x5e7340
                    || m.r32(class + 12) != jvm
                    || m.r32(class + 16) != 0
                {
                    return Err(
                        "NativeVmLive: ordinary object source construction owner differs".into(),
                    );
                }
                let token = self.token()?;
                self.physical.insert(
                    address,
                    Object {
                        token,
                        address,
                        class,
                        fields: Vec::new(),
                        array,
                        committed: false,
                    },
                );
                Ok(Some(Receipt::Object {
                    token,
                    address,
                    class,
                }))
            }
            Observation::InstanceField {
                object,
                index,
                variable,
                declaring,
                declared_index,
                record,
            } => {
                let owner = self
                    .physical
                    .get(&object)
                    .ok_or("NativeVmLive: instance field precedes its source object")?;
                let storage = m.r32(object + 16);
                let vector = m.r32(storage + 12);
                let members = m.r32(m.r32(declaring + 4) + 0x4c);
                if owner.committed
                    || index != owner.fields.len()
                    || m.r32(vector + 8) as usize != index + 1
                    || m.r32(m.r32(vector + 4) + index as u32 * 4) != variable
                    || self.variables.contains_key(&variable)
                    || m.r32(variable) != 0x5e7344
                    || m.r32(variable + 8) != 0
                    || m.r32(declaring + 12) != jvm
                    || declared_index >= m.r32(members + 12) as usize
                    || m.r32(m.r32(members + 8) + declared_index as u32 * 4) != record
                {
                    return Err(
                        "NativeVmLive: instance variable differs from actual source append".into(),
                    );
                }
                let object_address = object;
                let array = owner.array.is_some();
                let object = owner.token;
                let descriptor = m.r32(variable + 4);
                let name = m.r32(variable + 12);
                let descriptor_bytes = bytes(m, descriptor)?;
                bytes(m, name)?;
                let token = self.token()?;
                let field = Field {
                    token,
                    address: variable,
                    object,
                    index,
                    descriptor,
                    name,
                    descriptor_bytes,
                    declaring,
                    declared_index,
                    record,
                };
                self.variables.insert(variable, token);
                self.physical
                    .get_mut(&object_address)
                    .unwrap()
                    .fields
                    .push(field.clone());
                Ok((!array).then_some(Receipt::Field(field)))
            }
            Observation::ObjectCommitted { address } => {
                let object = self
                    .physical
                    .get_mut(&address)
                    .ok_or("NativeVmLive: object commit has no source constructor")?;
                let storage = m.r32(address + 16);
                let vector = m.r32(storage + 12);
                if object.committed
                    || m.r32(address) != 0x5e7354
                    || m.r32(address + 12) != object.class
                    || m.r32(address + 0x14) != 0
                    || m.r32(storage) != 0x5e7338
                    || m.r32(storage + 16) != jvm
                    || m.r32(vector + 8) as usize
                        != object.fields.len() + usize::from(object.array.is_some())
                {
                    return Err(
                        "NativeVmLive: ordinary object source construction is incomplete".into(),
                    );
                }
                if let Some(array) = &mut object.array {
                    let hidden = m.r32(m.r32(vector + 4) + object.fields.len() as u32 * 4);
                    let elements = m.r32(hidden + 8);
                    if elements == 0
                        || hidden == 0
                        || self.variables.contains_key(&hidden)
                        || m.r32(hidden) != 0x5e7344
                        || m.r32(hidden + 12) != 0x607a28
                        || bytes(m, m.r32(hidden + 4))? != b"I"
                        || array.length == 0
                        || object.fields.len() != 1
                        || object.fields[0].descriptor_bytes != b"I"
                        || m.r32(object.fields[0].address + 8) != array.length
                        || bytes(m, object.fields[0].name)? != b"length"
                        || m.r32(m.r32(object.class + 8)) != array.descriptor
                        || m.r32(elements + 8) != array.length
                    {
                        return Err(
                            "NativeVmLive: array length/storage source owner differs".into()
                        );
                    }
                    let table = m.r32(elements + 4);
                    // Validate the complete source owner before publishing any
                    // generation or variable ownership. A rejected later element
                    // must leave this receipt retryable after evidence repair.
                    let mut issued = Vec::new();
                    let mut addresses = std::collections::HashSet::new();
                    let mut next = self.next;
                    for index in 0..array.length {
                        let variable = m.r32(table + index * 4);
                        if variable == 0
                            || self.variables.contains_key(&variable)
                            || !addresses.insert(variable)
                            || m.r32(variable) != 0x5e7344
                            || m.r32(variable + 8) != 0
                            || m.r32(variable + 12) != 0
                            || bytes(m, m.r32(variable + 4))? != array.descriptor_bytes[1..]
                        {
                            return Err(
                                "NativeVmLive: array element source construction differs".into()
                            );
                        }
                        next = next
                            .checked_add(1)
                            .ok_or("NativeVmLive: array generation exhausted")?;
                        issued.push((variable, next));
                    }
                    next = next
                        .checked_add(1)
                        .ok_or("NativeVmLive: hidden array generation exhausted")?;
                    self.next = next;
                    self.variables.extend(issued.iter().copied());
                    self.variables.insert(hidden, next);
                    array.elements = issued;
                    array.hidden = hidden;
                    array.storage = elements;
                }
                object.committed = true;
                Ok(None)
            }
            _ => Err("NativeVmLive: observation is not an object owner".into()),
        }
    }
    pub fn apply(
        &mut self,
        m: &Space,
        event: &Event,
        metadata: &Metadata,
        receipt: &Receipt,
    ) -> Result<(), String> {
        match (event, receipt) {
            (
                Event::ObjectAllocate {
                    word,
                    class_context,
                    array_descriptor,
                    array_length,
                },
                Receipt::Object {
                    token,
                    address,
                    class,
                },
            ) => {
                let source = self
                    .physical
                    .get(address)
                    .ok_or("NativeVmLive: issued object generation disappeared")?;
                if *word == 0
                    || self.bound.contains_key(word)
                    || self.semantic.contains_key(address)
                    || !source.committed
                    || source.token != *token
                    || match (&source.array, array_descriptor, array_length) {
                        (Some(array), Some(descriptor), Some(length)) => {
                            *class_context != 0
                                || descriptor.as_bytes() != array.descriptor_bytes
                                || *length != array.length as usize
                        }
                        (None, None, None) => metadata.class(*class_context)?.class != *class,
                        _ => true,
                    }
                {
                    return Err(
                        "NativeVmLive: object event differs from next source allocation".into(),
                    );
                }
                self.semantic.insert(*address, *word);
                self.bound.insert(
                    *word,
                    Binding {
                        token: *token,
                        address: *address,
                        class: *class,
                        fields: if source.array.is_some() {
                            source.fields.clone()
                        } else {
                            Vec::new()
                        },
                    },
                );
                Ok(())
            }
            (
                Event::HeapConstruct {
                    slot: HeapSlot::Object { word, slot },
                    value,
                },
                Receipt::Field(field),
            ) => {
                let object = self
                    .bound
                    .get_mut(word)
                    .ok_or("NativeVmLive: field precedes its issued object binding")?;
                if object.token != field.object
                    || *slot != field.index
                    || object.fields.len() != field.index
                    || value.payload != 0
                    || value.tag.name().as_bytes() != field.descriptor_bytes
                    || metadata.instance_field_from_raw(m, field.declaring, field.declared_index)?
                        != field.record
                {
                    return Err(
                        "NativeVmLive: field event differs from next source variable declaration"
                            .into(),
                    );
                }
                object.fields.push(field.clone());
                Ok(())
            }
            _ => Err("NativeVmLive: source object/field continuation unavailable".into()),
        }
    }
    pub fn object(&self, m: &Space, word: u32) -> Result<u32, String> {
        let binding = self
            .bound
            .get(&word)
            .ok_or("NativeVmLive: object has no issued raw binding")?;
        let source = self
            .physical
            .get(&binding.address)
            .ok_or("NativeVmLive: physical object owner is absent")?;
        if !source.committed
            || source.token != binding.token
            || source.address != binding.address
            || m.r32(binding.address) != 0x5e7354
            || m.r32(binding.address + 12) != binding.class
        {
            return Err("NativeVmLive: object source allocation generation changed".into());
        }
        Ok(binding.address)
    }
    /// Resolve only a previously bound source allocation. This inverse lookup
    /// validates the same generation and physical owner as the forward path;
    /// an address or a coincidentally equal payload cannot become a VM word.
    pub fn semantic(&self, m: &Space, address: u32) -> Result<u32, String> {
        let word = *self
            .semantic
            .get(&address)
            .ok_or("NativeVmLive: raw object has no issued semantic owner")?;
        if self.object(m, word)? != address {
            return Err(
                "NativeVmLive: inverse object owner differs from its source binding".into(),
            );
        }
        Ok(word)
    }
    pub fn field(
        &self,
        m: &Space,
        word: u32,
        index: usize,
        metadata: &Metadata,
    ) -> Result<u32, String> {
        let object = self.object(m, word)?;
        let field = self.bound[&word]
            .fields
            .get(index)
            .ok_or("NativeVmLive: instance field has no issued raw binding")?;
        let vector = m.r32(m.r32(object + 16) + 12);
        if metadata.instance_field_from_raw(m, field.declaring, field.declared_index)?
            != field.record
            || self.variables.get(&field.address) != Some(&field.token)
            || m.r32(vector + 8) as usize <= index
            || m.r32(m.r32(vector + 4) + index as u32 * 4) != field.address
            || m.r32(field.address) != 0x5e7344
            || m.r32(field.address + 4) != field.descriptor
            || m.r32(field.address + 12) != field.name
        {
            return Err("NativeVmLive: instance variable source allocation/owner changed".into());
        }
        Ok(field.address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn string(m: &mut Space, text: &[u8]) -> u32 {
        let p = m.alloc(text.len() as u32 + 1);
        for (i, byte) in text.iter().chain([0].iter()).enumerate() {
            m.w8(p + i as u32, *byte);
        }
        p
    }
    fn variable(m: &mut Space, descriptor: u32, payload: u32, name: u32) -> u32 {
        let p = m.alloc(16);
        for (offset, word) in [0x5e7344, descriptor, payload, name]
            .into_iter()
            .enumerate()
        {
            m.w32(p + offset as u32 * 4, word);
        }
        p
    }
    // The same source observations that ordinary construction emits. Only the
    // second array element is damaged after the actual field append receipt.
    fn array(m: &mut Space) -> (Objects, u32, u32, u32, u32, u32) {
        let jvm = m.alloc(0x80);
        let class = m.alloc(0x200);
        let module = m.alloc(0x80);
        let descriptor_cell = m.alloc(4);
        let descriptor = string(m, b"[I");
        let integer = string(m, b"I");
        let name = string(m, b"length");
        m.w32(descriptor_cell, descriptor);
        for (offset, word) in [(0, 0x5e7340), (4, module), (8, descriptor_cell), (12, jvm)] {
            m.w32(class + offset, word);
        }
        let members = m.alloc(16);
        let declarations = m.alloc(4);
        let record = m.alloc(32);
        m.w32(module + 0x4c, members);
        m.w32(members + 8, declarations);
        m.w32(members + 12, 1);
        m.w32(declarations, record);
        let object = m.alloc(0x20);
        let storage = m.alloc(20);
        let vector = m.alloc(12);
        let table = m.alloc(8);
        m.w32(object, 0x5e7354);
        m.w32(object + 12, class);
        m.w32(object + 16, storage);
        m.w32(storage, 0x5e7338);
        m.w32(storage + 12, vector);
        m.w32(storage + 16, jvm);
        m.w32(vector + 4, table);
        let mut objects = Objects::default();
        objects
            .observe(
                m,
                jvm,
                Observation::ObjectBegin {
                    address: object,
                    class,
                    length: 2,
                },
            )
            .unwrap();
        let length = variable(m, integer, 0, name);
        m.w32(table, length);
        m.w32(vector + 8, 1);
        objects
            .observe(
                m,
                jvm,
                Observation::InstanceField {
                    object,
                    index: 0,
                    variable: length,
                    declaring: class,
                    declared_index: 0,
                    record,
                },
            )
            .unwrap();
        m.w32(length + 8, 2);
        let elements = m.alloc(12);
        let data = m.alloc(8);
        m.w32(elements + 4, data);
        m.w32(elements + 8, 2);
        let first = variable(m, integer, 0, 0);
        let second = variable(m, integer, 0, 0);
        m.w32(data, first);
        m.w32(data + 4, second);
        let hidden = variable(m, integer, elements, 0x607a28);
        m.w32(table + 4, hidden);
        m.w32(vector + 8, 2);
        (objects, jvm, object, data, second, integer)
    }
    fn reject_second_element_and_retry(duplicate: bool) {
        let mut m = Space::new();
        let (mut objects, jvm, object, data, second, integer) = array(&mut m);
        let next = objects.next;
        let variables = objects.variables.clone();
        if duplicate {
            m.w32(data + 4, m.r32(data));
        } else {
            let float = string(&mut m, b"F");
            m.w32(second + 4, float);
        }
        let error = objects
            .observe(&m, jvm, Observation::ObjectCommitted { address: object })
            .err()
            .expect("malformed second element must be rejected");
        assert!(error.contains("array element"));
        assert_eq!(objects.next, next, "failed commit consumed generations");
        assert_eq!(
            objects.variables, variables,
            "failed commit registered an earlier element"
        );
        let source = &objects.physical[&object];
        assert!(!source.committed);
        let array = source.array.as_ref().unwrap();
        assert!(array.elements.is_empty());
        assert_eq!((array.hidden, array.storage), (0, 0));
        m.w32(data + 4, second);
        m.w32(second + 4, integer);
        objects
            .observe(&m, jvm, Observation::ObjectCommitted { address: object })
            .unwrap();
        assert_eq!(objects.next, next + 3);
        let source = &objects.physical[&object];
        assert!(source.committed);
        let array = source.array.as_ref().unwrap();
        assert_eq!(array.elements.len(), 2);
        assert_ne!(array.hidden, 0);
        assert_ne!(array.storage, 0);
        assert_eq!(objects.variables.len(), variables.len() + 3);
    }
    fn committed_array() -> (Space, Objects, Metadata, u32, u32) {
        let mut m = Space::new();
        let (mut objects, jvm, object, _, _, _) = array(&mut m);
        objects
            .observe(&m, jvm, Observation::ObjectCommitted { address: object })
            .unwrap();
        // Metadata is deliberately irrelevant to the synthetic array class:
        // its issued descriptor/length, rather than a semantic class, identify it.
        m.w32(jvm + 8, jvm);
        m.w32(jvm + 0x68, jvm);
        let metadata = Metadata::new(&m, jvm).unwrap();
        (m, objects, metadata, object, jvm)
    }
    fn bind_array(
        m: &Space,
        objects: &mut Objects,
        metadata: &Metadata,
        object: u32,
        word: u32,
    ) -> Result<(), String> {
        let source = &objects.physical[&object];
        let receipt = Receipt::Object {
            token: source.token,
            address: object,
            class: source.class,
        };
        objects.apply(
            m,
            &Event::ObjectAllocate {
                word,
                class_context: 0,
                array_descriptor: Some("[I".into()),
                array_length: Some(2),
            },
            metadata,
            &receipt,
        )
    }
    #[test]
    fn inverse_object_lookup_requires_unique_acknowledged_source_owner() {
        let (m, mut objects, metadata, object, _) = committed_array();
        assert!(objects.semantic(&m, object).is_err());
        // Physical pointers themselves have no semantic identity.
        assert!(objects.object(&m, object).is_err());
        let word = 17;
        bind_array(&m, &mut objects, &metadata, object, word).unwrap();
        assert_eq!(objects.semantic(&m, object).unwrap(), word);
        assert_eq!(objects.object(&m, word).unwrap(), object);
        let error = bind_array(&m, &mut objects, &metadata, object, word + 1).unwrap_err();
        assert!(error.contains("differs from next source allocation"));
        assert!(!objects.bound.contains_key(&(word + 1)));
        assert_eq!(objects.semantic(&m, object).unwrap(), word);
        assert_eq!((objects.bound.len(), objects.semantic.len()), (1, 1));
    }
    #[test]
    fn inverse_object_lookup_revalidates_physical_generation_and_class() {
        let (mut m, mut objects, metadata, object, _) = committed_array();
        bind_array(&m, &mut objects, &metadata, object, 17).unwrap();
        let class = m.r32(object + 12);
        m.w32(object + 12, class ^ 1);
        assert!(objects.semantic(&m, object).is_err());
        m.w32(object + 12, class);
        assert_eq!(objects.semantic(&m, object).unwrap(), 17);
        objects.physical.get_mut(&object).unwrap().token += 1;
        assert!(objects.semantic(&m, object).is_err());
    }
    #[test]
    fn malformed_second_array_element_preserves_ledger_and_can_retry() {
        reject_second_element_and_retry(false);
    }
    #[test]
    fn duplicate_second_array_element_preserves_ledger_and_can_retry() {
        reject_second_element_and_retry(true);
    }
}
