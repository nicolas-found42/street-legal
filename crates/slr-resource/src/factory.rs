//! Default GameType template/native instance creation over a declared resource
//! world. Compiled constructors, spatial children and external templates remain
//! explicit boundaries; no script constructor is entered by this native path.
use crate::node::{self, Container, GameInstance};
use slr_vm::vmclass::ClassId;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Root {
    pub resource_id: u32,
    pub container: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Allocate(u32),
    CopyName(String),
    Construct {
        kind: u32,
        id: u32,
        parent: Root,
        descriptor: [u32; 5],
    },
    Retain,
    Data,
    Call(&'static str),
    CacheHit {
        class: ClassId,
        id: u32,
    },
}
#[derive(Clone, Debug)]
pub struct Template {
    pub id: u32,
    pub container: u32,
    pub class: ClassId,
    pub quality: u32,
    pub words: [u32; 2],
    pub compiled_constructor: bool,
}
#[derive(Clone, Debug)]
pub struct Instance {
    pub id: u32,
    pub container: u32,
    pub template_link: u32,
    pub name: String,
    pub script: u32,
    pub flags: u32,
    pub quality: u32,
    pub words: [u32; 2],
}

/// Original ushort free stack. Reserving supplied IDs declares earlier
/// allocations in the controlled runtime profile; it does not recover history.
#[derive(Debug)]
pub struct RuntimeIds {
    next: u32,
    free: Vec<u16>,
    reserved: HashSet<u32>,
}
impl RuntimeIds {
    pub fn new(next: u32) -> Self {
        Self {
            next,
            free: vec![],
            reserved: HashSet::new(),
        }
    }
    pub fn reserve(&mut self, id: u32) {
        self.reserved.insert(id);
    }
    /// exe: FUN_00536ed0
    pub fn allocate(&mut self) -> u32 {
        loop {
            if self.free.is_empty() {
                for _ in 0..128 {
                    self.free.push(self.next as u16);
                    self.next = self.next.wrapping_add(1);
                }
            }
            let id = 0xffff0000 | u32::from(self.free.pop().expect("refilled ID stack"));
            if self.reserved.insert(id) {
                return id;
            }
        }
    }
    /// exe: FUN_00536f90
    pub fn recycle(&mut self, id: u32) {
        if id >> 16 == 0xffff {
            self.reserved.remove(&id);
            self.free.push(id as u16);
        }
    }
    pub fn remaining(&self) -> usize {
        self.free.len()
    }
    pub fn next(&self) -> u32 {
        self.next
    }
}

#[derive(Debug)]
pub struct Factory {
    pub events: Vec<Event>,
    pub templates: HashMap<u32, Template>,
    pub defaults: HashMap<ClassId, u32>,
    pub instances: HashMap<u32, Instance>,
    pub created_count: u32,
    /// Fresh type1 constructor starts at 0x21. Controlled callback probes can
    /// supply zero storage instead; this seam does not describe fresh objects.
    pub initial_instance_flags: u32,
}
impl Default for Factory {
    fn default() -> Self {
        Self {
            events: vec![],
            templates: HashMap::new(),
            defaults: HashMap::new(),
            instances: HashMap::new(),
            created_count: 0,
            initial_instance_flags: 0x21,
        }
    }
}
impl Factory {
    /// Construct a default template with the measured fresh type8 data defaults.
    /// The ID/node/name collaborators remain supplied, as in the original probe.
    /// exe: FUN_0053a1a0
    pub fn create_template(
        &mut self,
        root: Root,
        class: ClassId,
        name: &str,
        allocate: &mut impl FnMut() -> u32,
    ) -> Result<Option<Template>, String> {
        let id = allocate();
        self.events.push(Event::Allocate(id));
        if id == 0 {
            return Ok(None);
        }
        self.events.push(Event::CopyName(name.into()));
        self.events.push(Event::Construct {
            kind: 8,
            id,
            parent: root,
            descriptor: [8, id, 2, 0, 0],
        });
        let container = node::with_res(|host| {
            let mut c = Container::new(8, id);
            c.parent = root.container.unwrap_or(0);
            c.flags = 2;
            let c = host.add_container(c);
            host.bind_id(id, c);
            c
        });
        self.events.push(Event::Retain);
        self.events.push(Event::Data);
        let template = Template {
            id,
            container,
            class,
            quality: 1.0f32.to_bits(),
            words: [1.0f32.to_bits(), 0],
            compiled_constructor: false,
        };
        self.templates.insert(id, template.clone());
        Ok(Some(template))
    }
    /// The wrapper caches a default type per retained class record (+0x1d4).
    pub fn default_for_class(
        &mut self,
        root: Root,
        class: ClassId,
        name: &str,
        allocate: &mut impl FnMut() -> u32,
    ) -> Result<Template, String> {
        if let Some(id) = self.defaults.get(&class) {
            self.events.push(Event::CacheHit { class, id: *id });
            return self
                .templates
                .get(id)
                .cloned()
                .ok_or("cached template missing".into());
        }
        let t = self
            .create_template(root, class, name, allocate)?
            .ok_or("default GameType ID allocation returned zero")?;
        self.defaults.insert(class, t.id);
        Ok(t)
    }
    /// Existing-script branch: keep the template link, set the script backref,
    /// transfer the two gametype words and release the temporary instance link.
    /// Fresh default instances have no spatial children, geometry mode2 and no
    /// physics envelope: original post-create helpers perform no work there.
    /// exe: FUN_0053a2a0
    pub fn create_instance(
        &mut self,
        root: Root,
        request: u32,
        script: u32,
        name: &str,
        allocate: &mut impl FnMut() -> u32,
    ) -> Result<Option<Instance>, String> {
        if request == 0 {
            return Ok(None);
        }
        let template = self.templates.get(&request).cloned().ok_or_else(|| {
            format!("external GameType resource {request:#010x} is not installed")
        })?;
        if template.compiled_constructor {
            return Err("compiled GameType constructor is not installed".into());
        }
        if root.container.is_some() {
            return Err("parented native instance inheritance is not installed".into());
        }
        self.events.push(Event::Call("0x536ed0"));
        let id = allocate();
        if id == 0 {
            return Ok(None);
        }
        self.events.push(Event::Call("0x551120"));
        self.events.push(Event::Call(if root.container.is_some() {
            "0x536900"
        } else {
            "0x536890"
        }));
        let (container, template_link) = node::with_res(|h| {
            let mut c = Container::new(1, id);
            c.parent = root.container.unwrap_or(0);
            c.flags = self.initial_instance_flags | 0x200;
            c.instance = Some(GameInstance {
                script,
                ..GameInstance::default()
            });
            let c = h.add_container(c);
            h.bind_id(id, c);
            let retained = h.add_bound_node(request, template.container);
            (c, retained)
        });
        for call in [
            "0x544b90",
            "instance-data",
            "template-materialise",
            "0x5447d0",
            "gametype-data",
            "0x427210",
            "0x544b90",
            "0x4988a0",
            "0x498b10",
            "0x48ae40",
        ] {
            self.events.push(Event::Call(call));
        }
        // No Java/script allocation occurs here (unlike 53a5e0).
        let instance = Instance {
            id,
            container,
            template_link,
            name: name.chars().take(31).collect(),
            script,
            flags: self.initial_instance_flags | 0x200,
            quality: 1.0f32.to_bits(),
            words: template.words,
        };
        self.created_count = self.created_count.wrapping_add(1);
        self.instances.insert(id, instance.clone());
        Ok(Some(instance))
    }
}
