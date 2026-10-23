//! The class loader and linker: packages, classes, methods, fields.
//!
//! Ports `FUN_0040fe70` (boot: package roots + primitive descriptors),
//! `FUN_00411b20` (chunk loop: `CONS`/`FILD`/`MTHD`/`CLSS`/`TREE`),
//! `FUN_00410890` (load-or-get), `FUN_00410630` (load from file) and
//! `FUN_00410430` (read file + link + package check). Metadata linking
//! installs methods/fields (`FUN_004195b0`/`FUN_00419b60`). The bounded
//! `FUN_00403e80` class-initialization driver lives in [`crate::host`];
//! it resolves already-loaded dependencies and executes static initializers.
//! This does not establish whole-world loading/initialization. Dispatch resolves through
//! `FUN_00404910`/`FUN_00404d40`/`FUN_00405920` with assignability
//! `FUN_00405400`. All `[static]` (machine code read); oracles: Unicorn
//! running the exe's own interner/registrar/boot
//! (`re/oracle/vmclass_vectors.py`, local only) plus the stock corpus
//! (each supported shipped file loads and links in an isolated namespace).
//! This structural check does not establish a globally linked stock world;
//! bundled/standalone conflicting definitions require production selection
//! rules in #110. Installed class identities and constant pools are immutable.
//!
//! Name model: the exe interns every name through the string table
//! ([`crate::strings`]); the loader keeps owned spellings. A value cell's
//! type pointer ([`crate::values::TypeTag`]) is the full descriptor
//! (`I`, `F`, `V`, `L;`, `Ljava.lang.String;`, `[I`, ...).

use std::collections::HashMap;

use slr_fmt_tufa::{ClassRec, FieldRec, MethodRec, Module, PoolEntry};

/// A linked method: declaring class, name, signature, native flag, body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Method {
    /// Declaring class (dotted).
    pub class: String,
    /// Method name.
    pub name: String,
    /// Signature descriptor (`()V`, `(F)V`, ...).
    pub sig: String,
    /// Original MTHD flags: bit0x40 native, bit0x08 static, bit0x10 abstract.
    pub flags: u32,
    /// MTHD first (static) versus second (instance) vector. This is distinct
    /// from dispatch's static flag and retains original candidate ordering.
    pub static_vector: bool,
    /// Native bit0x40 (including static0x49); script bodies live in
    /// [`Method::body`].
    pub is_native: bool,
    /// Index of the `TREE` body (script methods only; the `MTHD` 4th word).
    pub body: u32,
    /// `true` for the `0x11` abstract/interface records (body
    /// `0xffffffff`, no TUFA body; the sole stock case is
    /// `java.lang.Runnable.run`).
    pub is_abstract: bool,
    /// Arity tag (the `MTHD` 5th word; meaning open).
    pub extra: u32,
}

/// A linked field: declaring class, name, type descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// Declaring class (dotted).
    pub class: String,
    /// Field name.
    pub name: String,
    /// Type descriptor (`I`, `F`, `Ljava.lang...;`, ...).
    pub ty: String,
    /// Static (`0x28` first word) vs instance (`0`).
    pub is_static: bool,
}

/// A linked class: identity, hierarchy, members, method bodies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Class {
    /// CLSS head1:0 class,1 interface; unknown values remain explicit metadata.
    pub kind: u32,
    /// Dotted name (`java.game.MainMenu`).
    pub name: String,
    /// Direct superclass from CLSS head3 (`UINT_MAX` means no superclass).
    pub base: Option<String>,
    /// Interface names (pool `Class` entries past the base).
    pub interfaces: Vec<String>,
    /// Declared methods (both `MTHD` arrays concatenated, file order).
    pub methods: Vec<Method>,
    /// Declared fields (both `FILD` arrays concatenated, file order).
    pub fields: Vec<Field>,
    /// Decoded `TREE` bodies; `methods[i].body` indexes into this.
    pub bodies: Vec<Vec<slr_fmt_tufa::Node>>,
}

/// A selected method record, including the inherited vector index returned by
/// 405920 and its log-and-continue ambiguity condition.
pub struct MethodMatch<'a> {
    pub method: &'a Method,
    pub defining: &'a str,
    pub index: u32,
    pub ambiguous: bool,
}

/// Package roots (`FUN_0040fe70` substring table + boot classes).
pub mod boot {
    /// The 8 `(directory, package)` mappings registered by `FUN_0040fe70`
    /// through `FUN_0041cfe0` (decoded from exe bytes at `0x60ab6c..`).
    pub const PACKAGES: [(&str, &str); 8] = [
        ("system\\scripts\\lang", "java.lang"),
        ("system\\scripts\\io", "java.io"),
        ("system\\scripts\\util", "java.util"),
        ("system\\scripts\\render", "java.render"),
        ("system\\scripts\\sound", "java.sound"),
        ("sl\\scripts\\game", "java.game"),
        ("parts\\scripts", "java.game.parts"),
        ("parts\\accessories\\scripts", "java.game.parts.accessories"),
    ];

    /// Classes `FUN_0040fe70` loads eagerly after the packages.
    pub const BOOT_CLASSES: [&str; 2] = ["java.lang.String", "java.util.resource.GameRef"];

    /// Primitive descriptors bound by `FUN_0040fe70` (`DAT_0062def0` etc.).
    pub const PRIMITIVES: [(&str, &str); 5] = [
        ("DAT_0062def0", "I"),
        ("DAT_0062e00c", "I"),
        ("DAT_0062deec", "F"),
        ("DAT_0062e000", "L;"),
        ("DAT_0062e010", "V"),
    ];

    /// The root package (`jvm+0x7c`, `&DAT_0060ab64` = `java`).
    pub const ROOT_PACKAGE: &str = "java";
}

/// Why a class failed to load or link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// `FUN_00410430`: the file could not be read (`loadClass failed`).
    Io(String),
    /// No `CONS`/`CLSS`/`TREE` chunk (the exe walks whatever is there;
    /// the port requires the full shape).
    Malformed(String),
    /// `FUN_00410890`: class not found and no file provides it.
    NotFound(String),
    /// `FUN_00411b20`: unknown chunk tag.
    UnknownChunk(String),
    /// `FUN_0041cfe0`: package conflict (multiple packages own the name).
    PackageConflict(String),
    /// Pool or record index out of range.
    BadIndex(&'static str),
}

/// The class loader: loaded classes by dotted name, package roots, and the
/// boot state from `FUN_0040fe70`.
///
/// exe: FUN_0040fe70 (boot) + FUN_00411b20 (chunk loop)
#[derive(Clone, Debug, Default)]
pub struct Loader {
    classes: HashMap<String, Class>,
    packages: Vec<(String, String)>,
    booted: bool,
    contexts: Vec<String>,
    pools: HashMap<String, Vec<PoolEntry>>,
    body_owners: Vec<(String, usize)>,
    field_initializers: HashMap<String, Vec<Option<usize>>>,
}

impl Loader {
    /// Runtime Class::isInheritedFrom is distinct from405400's metadata
    /// conversion cost. Kinds must match and interface recursion is boolean.
    pub fn is_inherited_from(&self, from: &str, to: &str) -> Result<bool, LoadError> {
        fn visit(
            loader: &Loader,
            from: &str,
            to: &str,
            path: &mut std::collections::HashSet<String>,
        ) -> Result<bool, LoadError> {
            if !path.insert(from.to_owned()) {
                return Err(LoadError::Malformed("cyclic runtime type graph".into()));
            }
            let source = loader
                .find(from)
                .ok_or_else(|| LoadError::NotFound(from.into()))?;
            let target = loader
                .find(to)
                .ok_or_else(|| LoadError::NotFound(to.into()))?;
            let result = if source.kind != target.kind {
                false
            } else if from == to {
                true
            } else if source.kind == 0 {
                if let Some(parent) = source.base.as_deref() {
                    visit(loader, parent, to, path)?
                } else {
                    false
                }
            } else {
                let mut result = false;
                for interface in source.interfaces.iter().rev() {
                    if visit(loader, interface, to, path)? {
                        result = true;
                        break;
                    }
                }
                result
            };
            path.remove(from);
            Ok(result)
        }
        visit(self, from, to, &mut Default::default())
    }
    /// Original405250's same-kind hierarchy cost. Interface recursion follows
    /// only the last parent and adds a boolean success value, preserving the
    /// observed false-positive compatibility of unrelated interface graphs.
    fn same_kind_cost(
        &self,
        from: &str,
        to: &str,
        path: &mut std::collections::HashSet<String>,
    ) -> Result<i32, LoadError> {
        if !path.insert(from.to_owned()) {
            return Err(LoadError::Malformed("cyclic type graph".into()));
        }
        let source = self
            .find(from)
            .ok_or_else(|| LoadError::NotFound(from.into()))?;
        let target = self
            .find(to)
            .ok_or_else(|| LoadError::NotFound(to.into()))?;
        let result = if source.kind != target.kind {
            -1
        } else if from == to {
            0
        } else if source.kind == 0 {
            let mut cursor = source.base.as_deref();
            let mut depth = 1;
            let mut visited = std::collections::HashSet::new();
            let mut result = -1;
            while let Some(name) = cursor {
                if !visited.insert(name) {
                    return Err(LoadError::Malformed("cyclic superclass graph".into()));
                }
                if name == to {
                    result = 100 + depth;
                    break;
                }
                cursor = self
                    .find(name)
                    .ok_or_else(|| LoadError::NotFound(name.into()))?
                    .base
                    .as_deref();
                depth += 1;
            }
            result
        } else if let Some(parent) = source.interfaces.last() {
            if parent == to {
                1
            } else {
                1 + i32::from(self.same_kind_cost(parent, to, path)? >= 0)
            }
        } else {
            -1
        };
        path.remove(from);
        Ok(result)
    }

    /// Loaded metadata's405400 conversion distance. This deliberately retains
    /// the original interface-to-class mismatch return0 and class-to-interface
    /// implementation cost0; it is not Java subtype ranking.
    pub fn class_conversion_cost(&self, from: &str, to: &str) -> Result<i32, LoadError> {
        let source = self
            .find(from)
            .ok_or_else(|| LoadError::NotFound(from.into()))?;
        let target = self
            .find(to)
            .ok_or_else(|| LoadError::NotFound(to.into()))?;
        if source.kind == target.kind {
            return self.same_kind_cost(from, to, &mut Default::default());
        }
        if source.kind != 0 {
            return Ok(0);
        }
        if target.kind != 1 {
            return Ok(-1);
        }
        let mut cursor = Some(from);
        let mut visited = std::collections::HashSet::new();
        while let Some(name) = cursor {
            if !visited.insert(name) {
                return Err(LoadError::Malformed("cyclic implementation graph".into()));
            }
            let class = self
                .find(name)
                .ok_or_else(|| LoadError::NotFound(name.into()))?;
            for interface in class.interfaces.iter().rev() {
                if interface == to
                    || self.same_kind_cost(interface, to, &mut Default::default())? >= 0
                {
                    return Ok(0);
                }
            }
            cursor = class.base.as_deref();
        }
        Ok(-1)
    }

    /// 417c30's directional descriptor cost, used by overload signatures.
    pub fn conversion_cost(&self, source: &str, target: &str) -> Result<i32, LoadError> {
        if source == target {
            return Ok(0);
        }
        let source_kind = source.as_bytes().first().copied();
        let target_kind = target.as_bytes().first().copied();
        Ok(match (source_kind, target_kind) {
            (Some(b'I'), Some(b'F')) => 101,
            (Some(b'F'), Some(b'I')) => 100,
            (Some(b'L' | b'['), Some(b'L' | b'[')) => {
                if source == "L;" {
                    100
                } else {
                    self.class_conversion_cost(bare_name(source), bare_name(target))?
                }
            }
            (Some(b'I' | b'F'), Some(b'L' | b'['))
                if self.same_kind_cost(
                    bare_name(target),
                    "java.lang.String",
                    &mut Default::default(),
                )? >= 0 =>
            {
                100
            }
            _ => -1,
        })
    }

    /// Search static then instance vectors in reverse, derived to base. A zero
    /// score breaks only its vector; the last tested score controls traversal
    /// to the next owner, as in405920. Return descriptors do not rank methods.
    pub fn resolve_overload(
        &self,
        class: &str,
        name: &str,
        actual: &[crate::values::TypeTag],
    ) -> Result<Option<MethodMatch<'_>>, LoadError> {
        let mut cursor = Some(class);
        let mut best = 1_000_000;
        let mut last = -1;
        let mut selected: Option<(&Method, &str, usize)> = None;
        let mut tie = None;
        let mut visited = std::collections::HashSet::new();
        while let Some(owner) = cursor {
            if last == 0 {
                break;
            }
            if !visited.insert(owner) {
                return Err(LoadError::Malformed("cyclic method graph".into()));
            }
            let class = self
                .find(owner)
                .ok_or_else(|| LoadError::NotFound(owner.into()))?;
            for static_vector in [true, false] {
                let candidates = class
                    .methods
                    .iter()
                    .filter(|method| method.static_vector == static_vector)
                    .enumerate()
                    .collect::<Vec<_>>();
                for (index, method) in candidates.into_iter().rev() {
                    if method.name != name {
                        continue;
                    }
                    let parameters = crate::host::parameter_types(&method.sig)
                        .map_err(|_| LoadError::Malformed("method signature".into()))?;
                    last = if parameters.len() != actual.len() {
                        -1
                    } else {
                        let mut sum = 0;
                        for (source, target) in actual.iter().zip(&parameters).rev() {
                            let score = self.conversion_cost(source.name(), target.name())?;
                            if score < 0 {
                                sum = -1;
                                break;
                            }
                            sum += score;
                        }
                        sum
                    };
                    if last == 0 {
                        best = 0;
                        selected = Some((method, class.name.as_str(), index));
                        break;
                    }
                    if last >= 0 {
                        if last < best {
                            best = last;
                            selected = Some((method, class.name.as_str(), index))
                        } else if last == best {
                            tie = Some((last, class.name.as_str()));
                        }
                    }
                }
            }
            cursor = class.base.as_deref();
        }
        let Some((method, defining, index)) = selected else {
            return Ok(None);
        };
        let mut inherited = index as u32;
        let mut cursor = self.find(defining).and_then(|class| class.base.as_deref());
        let mut visited = std::collections::HashSet::new();
        while let Some(owner) = cursor {
            if !visited.insert(owner) {
                return Err(LoadError::Malformed("cyclic method index graph".into()));
            }
            let class = self
                .find(owner)
                .ok_or_else(|| LoadError::NotFound(owner.into()))?;
            inherited += class
                .methods
                .iter()
                .filter(|m| m.static_vector == method.static_vector)
                .count() as u32;
            cursor = class.base.as_deref();
        }
        Ok(Some(MethodMatch {
            method,
            defining,
            index: inherited | if method.static_vector { 0x4000_0000 } else { 0 },
            ambiguous: tie == Some((best, defining)),
        }))
    }
    /// An empty loader (a zeroed JVM object before `FUN_0040fe70`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Boot the loader (`FUN_0040fe70`, thiscall with the JVM context):
    /// bind the 5 primitive descriptors, set the `java` root package, and
    /// register the 8 package mappings. The 2 eager class loads need files
    /// and happen on first use instead (see [`Loader::boot_classes`]).
    ///
    /// exe: FUN_0040fe70
    pub fn boot(&mut self) {
        self.packages.clear();
        for (dir, pkg) in boot::PACKAGES {
            self.packages.push((dir.to_owned(), pkg.to_owned()));
        }
        self.booted = true;
    }

    /// `true` once [`Loader::boot`] ran.
    #[must_use]
    pub fn is_booted(&self) -> bool {
        self.booted
    }

    /// The package mappings (`(directory, package)`).
    #[must_use]
    pub fn packages(&self) -> &[(String, String)] {
        &self.packages
    }

    /// Number of loaded classes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.classes.len()
    }

    /// `true` when no class is loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }

    /// Look up a loaded class (`FUN_00411610` hit path).
    ///
    /// exe: FUN_00411610
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Class> {
        self.classes.get(name)
    }

    /// Insert a linked class directly (test seam for synthetic classes;
    /// production code uses [`Loader::load_module`]). Identical installation
    /// is idempotent; conflicting replacement panics before changing live IDs.
    pub fn insert_test_class(&mut self, class: Class) {
        let name = class.name.clone();
        if let Some(existing) = self.classes.get(&name) {
            assert_eq!(existing, &class, "cannot replace an installed class");
            return;
        }
        if !self.contexts.contains(&name) {
            self.contexts.push(name.clone());
        }
        for index in 0..class.bodies.len() {
            self.body_owners.push((name.clone(), index));
        }
        self.classes.insert(name, class);
    }

    /// Stable loaded-class context used by interpreter activation frames.
    pub fn context_word(&self, class: &str) -> Option<u32> {
        self.contexts
            .iter()
            .position(|name| name == class)
            .map(|index| index as u32 + 1)
    }

    /// Class owning an activation-frame context.
    pub fn context_name(&self, context: u32) -> Option<&str> {
        self.contexts
            .get(context.checked_sub(1)? as usize)
            .map(String::as_str)
    }

    /// Map a class-local TREE index to its executable body identity.
    pub fn body_id(&self, class: &str, index: usize) -> Option<crate::interp::BodyId> {
        self.body_owners
            .iter()
            .position(|(owner, local)| owner == class && *local == index)
            .map(|index| crate::interp::BodyId(index as u32))
    }

    /// Loaded TREE bytes. Body IDs retain their owning constant pool.
    pub fn body(&self, id: crate::interp::BodyId) -> Option<&[slr_fmt_tufa::Node]> {
        let (class, index) = self.body_owners.get(id.0 as usize)?;
        self.classes
            .get(class)?
            .bodies
            .get(*index)
            .map(Vec::as_slice)
    }

    /// Owner of a loaded TREE body.
    pub fn body_class(&self, id: crate::interp::BodyId) -> Option<&str> {
        self.body_owners
            .get(id.0 as usize)
            .map(|(class, _)| class.as_str())
    }

    /// Constant pool retained from the owning TUFA module.
    pub fn pool(&self, class: &str) -> Option<&[PoolEntry]> {
        self.pools.get(class).map(Vec::as_slice)
    }

    /// Preserve each FILD record's optional TREE initializer index.
    pub fn field_initializer(&self, field: &Field) -> Option<crate::interp::BodyId> {
        let index = self
            .find(&field.class)?
            .fields
            .iter()
            .position(|candidate| candidate == field)?;
        let local = (*self.field_initializers.get(&field.class)?.get(index)?)?;
        self.body_id(&field.class, local)
    }

    /// Read exact type0 constant bytes for a String literal (406140).
    /// Other pool kinds return null as406140 does. Class indirection belongs
    /// to metadata lookup, not literal byte access.
    /// exe: FUN_00406140
    pub fn pool_bytes(&self, class: &str, index: u32) -> Result<Option<&[u8]>, LoadError> {
        let pool = self
            .pool(class)
            .ok_or(LoadError::BadIndex("literal pool"))?;
        if (index as i32) < 0 {
            return Ok(None);
        }
        let entry = pool
            .get(index as usize)
            .ok_or(LoadError::BadIndex("literal index"))?;
        Ok(match entry {
            PoolEntry::Str(text) => Some(text.as_bytes()),
            PoolEntry::RawStr(bytes) => Some(bytes),
            _ => None,
        })
    }

    /// Read UTF-8 metadata, following explicit Class records by pool index.
    /// Raw string payloads cannot become metadata through replacement text.
    pub fn pool_text(&self, class: &str, mut index: u32) -> Result<&str, LoadError> {
        let pool = self.pool(class).ok_or(LoadError::BadIndex("pool"))?;
        let mut visited = std::collections::HashSet::new();
        loop {
            if !visited.insert(index) {
                return Err(LoadError::BadIndex("pool cycle"));
            }
            match pool.get(index as usize) {
                Some(PoolEntry::Str(text)) => return Ok(text),
                Some(PoolEntry::Class(next)) => index = *next,
                _ => return Err(LoadError::BadIndex("pool string")),
            }
        }
    }

    /// Decode a field reference's class and NameType name/type descriptors.
    /// exe: FUN_00406d00
    pub fn field_reference(
        &self,
        owner: &str,
        index: u32,
    ) -> Result<(&str, &str, &str), LoadError> {
        let pool = self.pool(owner).ok_or(LoadError::BadIndex("pool"))?;
        let Some(PoolEntry::Type5 { a, b }) = pool.get(index as usize) else {
            return Err(LoadError::BadIndex("field ref"));
        };
        let Some(PoolEntry::Type7 { a: name, b: ty }) = pool.get(*b as usize) else {
            return Err(LoadError::BadIndex("name type"));
        };
        Ok((
            self.pool_text(owner, *a)?,
            self.pool_text(owner, *name)?,
            self.pool_text(owner, *ty)?,
        ))
    }

    /// Install a decoded body and pool in an already linked class. This is
    /// also the data-free embedding seam for interpreter programs.
    pub fn add_body(
        &mut self,
        class: &str,
        nodes: Vec<slr_fmt_tufa::Node>,
        pool: Vec<PoolEntry>,
    ) -> Result<crate::interp::BodyId, LoadError> {
        let loaded = self
            .classes
            .get_mut(class)
            .ok_or_else(|| LoadError::NotFound(class.to_owned()))?;
        if self
            .pools
            .get(class)
            .is_some_and(|existing| !same_pool(existing, &pool))
        {
            return Err(LoadError::Malformed(format!(
                "conflicting pool for {class}"
            )));
        }
        let index = loaded.bodies.len();
        loaded.bodies.push(nodes);
        self.pools.entry(class.to_owned()).or_insert(pool);
        let id = crate::interp::BodyId(self.body_owners.len() as u32);
        self.body_owners.push((class.to_owned(), index));
        Ok(id)
    }

    /// Load-or-get (`FUN_00410890`): `[...` goes to the array path (open:
    /// arrays are not modelled, reported as [`LoadError::NotFound`]);
    /// `L...;` strips to the bare name; a loaded class hits; otherwise the
    /// miss must be resolved from a file by the caller
    /// ([`Loader::load_module`]).
    ///
    /// exe: FUN_00410890
    pub fn get_or_missing(&self, descriptor: &str) -> Result<&Class, LoadError> {
        let name = bare_name(descriptor);
        self.classes
            .get(name)
            .ok_or_else(|| LoadError::NotFound(name.to_owned()))
    }

    /// Link one parsed module: `CONS` strings, `FILD`/`MTHD` records,
    /// `CLSS` hierarchy, `TREE` bodies (`FUN_00411b20` case order).
    ///
    /// exe: FUN_00411b20
    pub fn load_module(&mut self, module: &Module, data: &[u8]) -> Result<String, LoadError> {
        let pool = module
            .pool(data)
            .map_err(|e| LoadError::Malformed(e.to_string()))?;
        let strings = pool_strings(&pool);
        let class_rec: ClassRec = module
            .class_rec(data)
            .map_err(|e| LoadError::Malformed(e.to_string()))?;
        let name = pool_str(&pool, &strings, class_rec.head[2])?.to_owned();
        let base = if class_rec.head[3] == u32::MAX {
            None
        } else {
            Some(pool_str(&pool, &strings, class_rec.head[3])?.to_owned())
        };
        let interfaces = class_interfaces(&class_rec, &pool, &strings)?;
        let fields = link_fields(module, data, &pool, &strings, &name)?;
        let methods = link_methods(module, data, &pool, &strings, &name)?;
        let bodies = module
            .trees(data)
            .map_err(|e| LoadError::Malformed(e.to_string()))?;
        for m in &methods {
            // `0x11` (abstract/interface, e.g. `Runnable.run`) carries body
            // `0xffffffff` with zero TREE bodies; the exe never calls it
            // (dispatch misses are fatal upstream). Only concrete script
            // methods must index a real body.
            if !m.is_native && !m.is_abstract && (m.body as usize) >= bodies.len() {
                return Err(LoadError::BadIndex("method body"));
            }
        }
        let initializers: Vec<Option<usize>> = module
            .fields(data)
            .map_err(|error| LoadError::Malformed(error.to_string()))?
            .map(|arrays| {
                arrays
                    .into_iter()
                    .flatten()
                    .map(|record| {
                        if record[3] == u32::MAX {
                            Ok(None)
                        } else if record[3] as usize >= bodies.len() {
                            Err(LoadError::BadIndex("field initializer"))
                        } else {
                            Ok(Some(record[3] as usize))
                        }
                    })
                    .collect::<Result<Vec<_>, LoadError>>()
            })
            .transpose()?
            .unwrap_or_default();
        let linked = Class {
            kind: class_rec.head[1],
            name: name.clone(),
            base,
            interfaces,
            methods,
            fields,
            bodies,
        };
        if let Some(existing) = self.classes.get(&name) {
            if existing == &linked
                && self
                    .pools
                    .get(&name)
                    .is_some_and(|old| same_pool(old, &pool))
                && self.field_initializers.get(&name) == Some(&initializers)
            {
                return Ok(name);
            }
            return Err(LoadError::Malformed(format!("conflicting class {name}")));
        }
        self.field_initializers.insert(name.clone(), initializers);
        self.pools.insert(name.clone(), pool);
        self.insert_test_class(linked);
        Ok(name)
    }

    /// Load every module of one `.class` file (`FUN_00410430` over the
    /// `TUFA`-split modules of [`slr_fmt_tufa::parse`]).
    ///
    /// exe: FUN_00410430
    pub fn load_file(&mut self, data: &[u8]) -> Result<Vec<String>, LoadError> {
        let modules = slr_fmt_tufa::parse(data).map_err(|e| LoadError::Malformed(e.to_string()))?;
        modules.iter().map(|m| self.load_module(m, data)).collect()
    }

    /// Resolve `(class, name, sig)` to `(method, defining class)` walking
    /// the superclass chain (`FUN_00404910` + `FUN_00404d40` shape: exact
    /// name+sig match, inherited methods visible).
    ///
    /// exe: FUN_00404910
    #[must_use]
    pub fn resolve_method(&self, class: &str, name: &str, sig: &str) -> Option<(&Method, String)> {
        let mut cursor = Some(class.to_owned());
        while let Some(cname) = cursor {
            let class = self.classes.get(&cname)?;
            if let Some(m) = class
                .methods
                .iter()
                .find(|m| m.name == name && m.sig == sig)
            {
                return Some((m, cname));
            }
            cursor = class.base.clone();
        }
        None
    }

    /// Assignability: can a value of `from` be stored into `to`
    /// (`FUN_00405400` nonnegative compatibility cost)? The recovered
    /// interface cases include original non-Java compatibility quirks; this
    /// is not a generic Java assignability predicate.
    ///
    /// exe: FUN_00405400
    #[must_use]
    pub fn is_assignable(&self, from: &str, to: &str) -> bool {
        self.class_conversion_cost(from, to)
            .is_ok_and(|cost| cost >= 0)
    }
}

// A class owns one immutable pool. Float constants compare their serialized
// bits, preserving NaN payloads and distinguishing positive/negative zero.
fn same_pool(left: &[PoolEntry], right: &[PoolEntry]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| match (left, right) {
                (PoolEntry::Float(left), PoolEntry::Float(right)) => {
                    left.to_bits() == right.to_bits()
                }
                _ => left == right,
            })
}

/// Strip `L...;` to the bare class name; other descriptors pass through
/// (`FUN_00410890` header).
fn bare_name(descriptor: &str) -> &str {
    if let Some(inner) = descriptor
        .strip_prefix('L')
        .and_then(|s| s.strip_suffix(';'))
    {
        inner
    } else {
        descriptor
    }
}

fn pool_strings(pool: &[PoolEntry]) -> Vec<String> {
    pool.iter()
        .filter_map(|e| match e {
            PoolEntry::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn pool_str<'a>(
    pool: &'a [PoolEntry],
    _strings: &'a [String],
    mut idx: u32,
) -> Result<&'a str, LoadError> {
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(idx) {
            return Err(LoadError::BadIndex("class name cycle"));
        }
        match pool.get(idx as usize) {
            Some(PoolEntry::Str(text)) => return Ok(text),
            Some(PoolEntry::Class(next)) => idx = *next,
            _ => return Err(LoadError::BadIndex("pool string")),
        }
    }
}

fn class_interfaces(
    rec: &ClassRec,
    pool: &[PoolEntry],
    strings: &[String],
) -> Result<Vec<String>, LoadError> {
    // CLSS head: [base_idx, ?, ?, ?, tail_count]; tail holds interface
    // class indices (observed: WheelRef `0` tail = no interfaces;
    // MainMenu tail `[5]` = pool class 5 = java.game.GameState).
    rec.tail
        .iter()
        .map(|index| pool_str(pool, strings, *index).map(str::to_owned))
        .collect()
}

fn link_fields(
    module: &Module,
    data: &[u8],
    pool: &[PoolEntry],
    strings: &[String],
    class: &str,
) -> Result<Vec<Field>, LoadError> {
    // FILD: two arrays of `(flags, name_idx, type_idx, serial)`; arr0 holds
    // the `0x28` static records, arr1 the instance fields.
    let mut out = Vec::new();
    let arrays = module
        .fields(data)
        .map_err(|e| LoadError::Malformed(e.to_string()))?;
    if let Some([arr0, arr1]) = arrays {
        for rec in arr0.iter().chain(arr1.iter()) {
            out.push(link_field(rec, pool, strings, class)?);
        }
    }
    Ok(out)
}

fn link_field(
    rec: &FieldRec,
    pool: &[PoolEntry],
    strings: &[String],
    class: &str,
) -> Result<Field, LoadError> {
    let name = pool_str(pool, strings, rec[1])?.to_owned();
    let ty = pool_str(pool, strings, rec[2])?.to_owned();
    Ok(Field {
        class: class.to_owned(),
        name,
        ty,
        is_static: rec[0] == 0x28,
    })
}

fn link_methods(
    module: &Module,
    data: &[u8],
    pool: &[PoolEntry],
    strings: &[String],
    class: &str,
) -> Result<Vec<Method>, LoadError> {
    // 411b20 loads the first MTHD array into CCD+58 (static), then the
    // second into CCD+50(instance). Preserve each vector's serialized order.
    let mut out = Vec::new();
    let arrays = module
        .methods(data)
        .map_err(|e| LoadError::Malformed(e.to_string()))?;
    if let Some([arr0, arr1]) = arrays {
        for (records, static_vector) in [(&arr0, true), (&arr1, false)] {
            for rec in records.iter() {
                out.push(link_method(rec, pool, strings, class, static_vector)?);
            }
        }
    }
    Ok(out)
}

fn link_method(
    rec: &MethodRec,
    pool: &[PoolEntry],
    strings: &[String],
    class: &str,
    static_vector: bool,
) -> Result<Method, LoadError> {
    let name = pool_str(pool, strings, rec[1])?.to_owned();
    let sig = pool_str(pool, strings, rec[2])?.to_owned();
    Ok(Method {
        class: class.to_owned(),
        name,
        sig,
        flags: rec[0],
        static_vector,
        is_native: rec[0] & 0x40 != 0,
        body: rec[3],
        is_abstract: rec[0] & 0x10 != 0,
        extra: rec[4],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_module(
        mut pool: Vec<PoolEntry>,
        methods: Vec<MethodRec>,
        fields: Vec<FieldRec>,
    ) -> (Module, Vec<u8>) {
        // Build a minimal CONS/MTHD/FILD/CLSS/TREE image by hand. Layout:
        // TUFA(4) CONS MTHD FILD CLSS TREE, single module.
        let self_class = pool.len() as u32;
        pool.push(PoolEntry::Class(0));
        let base_class = pool.len() as u32;
        pool.push(PoolEntry::Class(1));
        let mut cons = Vec::new();
        cons.extend_from_slice(&(pool.len() as u32).to_le_bytes());
        let mut strings: Vec<String> = Vec::new();
        for e in &pool {
            match e {
                PoolEntry::Str(s) => {
                    cons.extend_from_slice(&0u32.to_le_bytes());
                    cons.extend_from_slice(&(s.len() as u32).to_le_bytes());
                    cons.extend_from_slice(s.as_bytes());
                    cons.push(0);
                    strings.push(s.clone());
                }
                PoolEntry::RawStr(bytes) => {
                    cons.extend_from_slice(&0u32.to_le_bytes());
                    cons.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    cons.extend_from_slice(bytes);
                    cons.push(0);
                }
                PoolEntry::Int(v) => {
                    cons.extend_from_slice(&1u32.to_le_bytes());
                    cons.extend_from_slice(&(*v as u32).to_le_bytes());
                }
                PoolEntry::Float(v) => {
                    cons.extend_from_slice(&2u32.to_le_bytes());
                    cons.extend_from_slice(&v.to_bits().to_le_bytes());
                }
                PoolEntry::Member { name, extra } => {
                    cons.extend_from_slice(&3u32.to_le_bytes());
                    cons.extend_from_slice(&name.to_le_bytes());
                    cons.extend_from_slice(&extra.to_le_bytes());
                }
                PoolEntry::Class(c) => {
                    cons.extend_from_slice(&4u32.to_le_bytes());
                    cons.extend_from_slice(&c.to_le_bytes());
                }
                PoolEntry::Type5 { a, b } => {
                    cons.extend_from_slice(&5u32.to_le_bytes());
                    cons.extend_from_slice(&a.to_le_bytes());
                    cons.extend_from_slice(&b.to_le_bytes());
                }
                PoolEntry::Type7 { a, b } => {
                    cons.extend_from_slice(&7u32.to_le_bytes());
                    cons.extend_from_slice(&a.to_le_bytes());
                    cons.extend_from_slice(&b.to_le_bytes());
                }
            }
        }
        let mut mthd = Vec::new();
        mthd.extend_from_slice(&0u32.to_le_bytes());
        mthd.extend_from_slice(&(methods.len() as u32).to_le_bytes());
        for r in &methods {
            for w in r {
                mthd.extend_from_slice(&w.to_le_bytes());
            }
        }
        let mut fild = Vec::new();
        fild.extend_from_slice(&0u32.to_le_bytes());
        fild.extend_from_slice(&(fields.len() as u32).to_le_bytes());
        for r in &fields {
            for w in r {
                fild.extend_from_slice(&w.to_le_bytes());
            }
        }
        // CLSS self/parent words are actual Class pool entries, not filtered
        // string positions. Keep existing member indices stable in this fixture.
        let mut clss = Vec::new();
        for w in [u32::MAX, 0, self_class, base_class, 0] {
            clss.extend_from_slice(&w.to_le_bytes());
        }
        // TREE: one empty body per script method (body idx 0..).
        let bodies = methods.iter().filter(|r| r[0] != 0x41).count().max(1);
        let mut tree = Vec::new();
        tree.extend_from_slice(&(bodies as u32).to_le_bytes());
        for _ in 0..bodies {
            tree.extend_from_slice(&0u32.to_le_bytes());
        }
        let mut data = Vec::new();
        fn chunk(data: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
            data.extend_from_slice(tag);
            data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            data.extend_from_slice(payload);
        }
        data.extend_from_slice(b"TUFA");
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&0x11ae1u32.to_le_bytes());
        chunk(&mut data, b"CONS", &cons);
        chunk(&mut data, b"FILD", &fild);
        chunk(&mut data, b"MTHD", &mthd);
        chunk(&mut data, b"CLSS", &clss);
        chunk(&mut data, b"TREE", &tree);
        let modules = slr_fmt_tufa::parse(&data).expect("synthetic parses");
        assert_eq!(modules.len(), 1);
        (modules.into_iter().next().unwrap(), data)
    }

    fn sample_pool() -> Vec<PoolEntry> {
        vec![
            PoolEntry::Str("game.Widget".into()),
            PoolEntry::Str("java.lang.Object".into()),
            PoolEntry::Str("I".into()),
            PoolEntry::Str("count".into()),
            PoolEntry::Str("tick".into()),
            PoolEntry::Str("()V".into()),
            PoolEntry::Str("get".into()),
            PoolEntry::Str("()I".into()),
        ]
    }

    #[test]
    fn boot_registers_packages() {
        let mut l = Loader::new();
        assert!(!l.is_booted());
        l.boot();
        assert!(l.is_booted());
        assert_eq!(l.packages().len(), 8);
        assert!(l.packages().iter().any(|(_, p)| p == "java.lang"));
        assert!(l
            .packages()
            .iter()
            .any(|(_, p)| p == "java.game.parts.accessories"));
        assert_eq!(boot::BOOT_CLASSES.len(), 2);
    }

    #[test]
    fn load_module_links_members_and_bodies() {
        // pool: 0 class, 1 base, 2 "I", 3 "count", 4 "tick", 5 "()V", 6 "get", 7 "()I"
        let pool = sample_pool();
        // methods: tick script body 0, get native.
        let methods = vec![[1u32, 4, 5, 0, 2], [0x41u32, 6, 7, 1, 0]];
        let fields = vec![[0u32, 3, 2, 0xffffffff]];
        let (module, data) = tiny_module(pool, methods, fields);
        let mut l = Loader::new();
        l.boot();
        let name = l.load_module(&module, &data).expect("links");
        assert_eq!(name, "game.Widget");
        let c = l.find("game.Widget").expect("loaded");
        assert_eq!(c.base.as_deref(), Some("java.lang.Object"));
        assert_eq!(c.methods.len(), 2);
        assert!(!c.methods[0].is_native);
        assert_eq!(c.methods[0].body, 0);
        assert!(c.methods[1].is_native);
        assert_eq!(c.fields.len(), 1);
        assert_eq!(c.fields[0].name, "count");
        assert_eq!(c.fields[0].ty, "I");
        assert!(!c.fields[0].is_static);
        // L...; lookup strips to the bare name.
        assert!(l.get_or_missing("Lgame.Widget;").is_ok());
        assert!(l.get_or_missing("Lgame.Missing;").is_err());
    }

    #[test]
    fn invalid_field_initializer_is_rejected_before_installation() {
        let (module, data) = tiny_module(sample_pool(), vec![], vec![[0, 3, 2, 99]]);
        let mut loader = Loader::new();
        assert_eq!(
            loader.load_module(&module, &data),
            Err(LoadError::BadIndex("field initializer"))
        );
        assert!(loader.find("game.Widget").is_none());
        assert!(loader.pool("game.Widget").is_none());
        assert!(loader.context_word("game.Widget").is_none());
    }

    #[test]
    fn class_reload_is_idempotent_and_conflicts_leave_existing_metadata_unchanged() {
        let (module, data) = tiny_module(sample_pool(), vec![], vec![[0, 3, 2, u32::MAX]]);
        let mut loader = Loader::new();
        loader.load_module(&module, &data).unwrap();
        let context = loader.context_word("game.Widget").unwrap();
        let body = loader.body_id("game.Widget", 0).unwrap();
        loader.load_module(&module, &data).unwrap();
        assert_eq!(loader.context_word("game.Widget"), Some(context));
        assert_eq!(loader.body_id("game.Widget", 0), Some(body));
        let mut pool = sample_pool();
        pool[3] = PoolEntry::Str("different".into());
        let (replacement, replacement_data) = tiny_module(pool, vec![], vec![[0, 3, 2, u32::MAX]]);
        assert!(loader.load_module(&replacement, &replacement_data).is_err());
        assert_eq!(loader.pool_text("game.Widget", 3).unwrap(), "count");
        assert_eq!(loader.find("game.Widget").unwrap().fields[0].name, "count");
        assert_eq!(loader.body_id("game.Widget", 0), Some(body));
    }

    #[test]
    fn method_resolution_walks_the_chain() {
        let pool = sample_pool();
        let (module, data) = tiny_module(pool, vec![[1u32, 4, 5, 0, 2]], vec![]);
        let mut l = Loader::new();
        l.boot();
        // Parent first so the child can point at it.
        l.classes.insert(
            "java.lang.Object".into(),
            Class {
                kind: 0,
                name: "java.lang.Object".into(),
                base: None,
                interfaces: vec![],
                methods: vec![],
                fields: vec![],
                bodies: vec![],
            },
        );
        l.load_module(&module, &data).expect("links");
        let (m, def) = l
            .resolve_method("game.Widget", "tick", "()V")
            .expect("found");
        assert_eq!(def, "game.Widget".to_owned());
        assert_eq!(m.name, "tick");
        assert!(l.resolve_method("game.Widget", "nope", "()V").is_none());
        assert!(l.is_assignable("game.Widget", "game.Widget"));
        assert!(l.is_assignable("game.Widget", "java.lang.Object"));
        assert!(!l.is_assignable("java.lang.Object", "game.Widget"));
    }
}
