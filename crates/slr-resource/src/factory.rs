//! GameType template/native instance creation over a declared resource world.
//! Script-only text and the fresh-empty loaded parent branch are supported.
//! Compiled constructors and spatial/physics parent branches fail closed; no
//! script constructor is entered by this native path.
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
    pub class: Option<ClassId>,
    pub quality: u32,
    pub words: [u32; 2],
    pub compiled_constructor: bool,
    /// Engine-native class named by a `native <name>` descriptor line. The
    /// headless runtime creates no engine object for it (see `NATIVE_CLASSES`).
    pub native_class: Option<String>,
    /// Optional configuration filename supplied to the native constructor.
    pub native_config: Option<String>,
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
    pub range_bits: [u32; 2],
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
    /// Only the declared, already materialized, fresh empty type1 parent branch.
    pub fresh_empty_parents: HashSet<u32>,
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
            fresh_empty_parents: HashSet::new(),
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
            if let Some(parent) = root.container {
                host.append_child(parent, c)?;
            }
            Ok::<_, String>(c)
        })?;
        self.events.push(Event::Retain);
        self.events.push(Event::Data);
        let template = Template {
            id,
            container,
            class: Some(class),
            quality: 1.0f32.to_bits(),
            words: [1.0f32.to_bits(), 0],
            compiled_constructor: false,
            native_class: None,
            native_config: None,
        };
        self.templates.insert(id, template.clone());
        Ok(Some(template))
    }
    /// Install a decoded script-only type8 template over fresh constructor data.
    /// Package lookup/inheritance is supplied by the caller and remains logged.
    pub fn install_script_template(
        &mut self,
        id: u32,
        parent: Root,
        data: &ScriptData,
    ) -> Result<Template, String> {
        if let Some(t) = self.templates.get(&id) {
            return Ok(t.clone());
        }
        // A `script` line whose class does not load leaves +0x10 null: the
        // original logs "Missing class" and continues, so the template exists
        // without a script class.
        let class = data.class;
        let container = node::with_res(|h| {
            if let Some(existing) = h.container_for_id(id) {
                // A header registered from its package is adopted: the
                // descriptor parse that installs the script class is the
                // materialisation of that same container.
                return if h.container(existing).is_some_and(|c| c.type_id == 8) {
                    Ok(existing)
                } else {
                    Err("external template ID already belongs to another container".into())
                };
            }
            let mut c = Container::new(8, id);
            c.flags = 0;
            let c = h.add_container(c);
            h.bind_id(id, c);
            if let Some(parent) = parent.container {
                h.append_child(parent, c)?;
            }
            Ok::<_, String>(c)
        })?;
        let t = Template {
            id,
            container,
            class,
            quality: 1f32.to_bits(),
            words: data.words,
            compiled_constructor: false,
            native_class: data.native.clone(),
            native_config: data.native_config.clone(),
        };
        self.templates.insert(id, t.clone());
        Ok(t)
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
        self.create_instance_identity(root, request, script, name, allocate, false)
    }

    /// Allocate the resource identity for a configured instance before its
    /// native constructor runs. The embedding host must execute the actual
    /// constructor and must not report successful creation on a boundary.
    pub fn allocate_configured_instance(
        &mut self,
        root: Root,
        request: u32,
        script: u32,
        name: &str,
        allocate: &mut impl FnMut() -> u32,
    ) -> Result<Option<Instance>, String> {
        if self
            .templates
            .get(&request)
            .is_none_or(|template| template.native_config.is_none())
        {
            return Err("configured identity allocation requires a configured template".into());
        }
        self.create_instance_identity(root, request, script, name, allocate, true)
    }

    fn create_instance_identity(
        &mut self,
        root: Root,
        request: u32,
        script: u32,
        name: &str,
        allocate: &mut impl FnMut() -> u32,
        configured: bool,
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
        if template.native_config.is_some() && !configured {
            return Err("configured native GameType constructor is not installed".into());
        }
        // A loaded type1 parent is inherited from without any load work
        // (`piVar1[0x19] & 1` set skips the parent load calls); the two range
        // words flow through the setters at vtable +0x14/+0x18. Unloaded or
        // other-kind parents would take the parent-load branch, which is not
        // modelled.
        let mut inherited_range = [0, 0];
        if let Some(parent) = root.container {
            let loaded = node::with_res(|h| {
                h.container(parent)
                    .is_some_and(|c| c.type_id == 1 && c.load_state & 1 != 0)
            });
            if !loaded {
                return Err(
                    "parented native instance inheritance requires a loaded type1 parent; the unloaded-parent load branch is not modelled"
                        .into(),
                );
            }
            if let Some(parent) = self.instances.values().find(|i| i.container == parent) {
                inherited_range = parent.range_bits;
            }
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
            // The factory's own load call (vtable +0xc with 1.0f) leaves the
            // instance loaded, which is what makes it an inheritable parent.
            c.load_state |= 1;
            c.instance = Some(GameInstance {
                script,
                ..GameInstance::default()
            });
            let c = h.add_container(c);
            h.bind_id(id, c);
            if let Some(parent) = root.container {
                h.append_child(parent, c)?;
            }
            let retained = h.add_bound_node(request, template.container);
            Ok::<_, String>((c, retained))
        })?;
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
            range_bits: inherited_range,
        };
        self.created_count = self.created_count.wrapping_add(1);
        self.instances.insert(id, instance.clone());
        Ok(Some(instance))
    }
}

/// Measured high-quality text parser branch over fresh type8 constructor data.
/// Unsupported descriptor commands fail closed before a native instance exists.
#[derive(Debug)]
pub struct ScriptData {
    pub class: Option<ClassId>,
    pub words: [u32; 2],
    pub native: Option<String>,
    pub native_config: Option<String>,
}
impl Default for ScriptData {
    fn default() -> Self {
        Self {
            class: None,
            words: [1f32.to_bits(), 0],
            native: None,
            native_config: None,
        }
    }
}

/// The engine's native game-object classes: the name table at `0x60c848`
/// (`ground`, `camera`, ... `building`), each paired in the exe with a compiled
/// constructor. Only the names are modelled.
pub const NATIVE_CLASSES: [&str; 15] = [
    "ground", "camera", "object", "car", "part", "trigger", "player", "button", "cursor", "bot",
    "road", "walk", "trlight", "ped", "building",
];
impl ScriptData {
    /// exe: FUN_005461a0
    pub fn apply(
        &mut self,
        text: &str,
        request: &mut impl FnMut(&str) -> Result<Option<ClassId>, String>,
    ) -> Result<(), String> {
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let mut words = line.split_whitespace();
            match (words.next(), words.next(), words.next()) {
                (Some("script"), Some(name), None) => self.class = request(name)?,
                (Some(key), _, _) if key.eq_ignore_ascii_case("lod_amp") => {
                    //5461a0 scans directly into payload+18. Failed conversions
                    //retain the inherited value; trailing text is not checked.
                    if let Some(value) = slr_fmt_cfg::scan::sscanf(
                        line.as_bytes(),
                        b"%*s %f",
                        slr_fmt_cfg::Precision::F32,
                    )
                    .f(0)
                    {
                        self.words[0] = value.to_bits();
                    }
                }
                //Type8 has no lod_bias branch:5461a0 ignores this key, unlike
                //the render/camera resource parsers that recognize it.
                (Some(key), _, _) if key.eq_ignore_ascii_case("lod_bias") => {}
                (Some("native"), Some(name), config) if NATIVE_CLASSES.contains(&name) => {
                    if words.next().is_some() {
                        return Err(format!("unsupported native descriptor tail {line:?}"));
                    }
                    self.native = Some(name.to_owned());
                    self.native_config = config.map(str::to_owned);
                }
                _ => return Err(format!("unsupported type8 descriptor line {line:?}")),
            }
        }
        Ok(())
    }
}
