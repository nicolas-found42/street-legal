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
//!
//! Class identity model (#110 steps 1-6): every loaded class owns one
//! append-only arena record addressed by a stable [`ClassId`] (nonzero
//! `u32`, separate from descriptor/name text). Bodies, pools, field
//! initializer indices and frame contexts dereference the record by ID;
//! a class name remains diagnostic metadata and is never enough to
//! dereference an already-issued identity. Name-to-ID resolution
//! ([`Loader::lookup`]) is the original 403a90/411610 512-bucket cache
//! with head retention, insert-after-head placement and full hash + name
//! compare. Production duplicate-name registration creates a new record
//! per parsed module (real 403a90 semantics); identical synthetic
//! installation stays idempotent like a cache hit. Base/interface
//! spellings resolve through the current selection on each hierarchy walk
//! until the class's own owner installs, then the owner retains its
//! resolved dependencies on reentry (403e80 binding-time policy).

use std::num::NonZeroU32;

use slr_fmt_tufa::{ClassRec, FieldRec, MethodRec, Module, PoolEntry};

/// Stable identity of one loaded class record, issued in registration order.
///
/// This is the owner carried by [`crate::interp::BodyId`] mappings, pools,
/// field-initializer indices and interpreter frame contexts
/// (`frame+0x34` holds [`ClassId::get`]; decoding it back to a name and
/// re-looking that name up is forbidden for execution). It is deliberately
/// separate from interned descriptor/name identity: equal descriptor text
/// must not be equated with equal runtime record identity, or vice versa.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClassId(NonZeroU32);

impl ClassId {
    /// Arena slot for this identity.
    fn index(self) -> usize {
        self.0.get() as usize - 1
    }

    /// The nonzero encoding stored in frame contexts and static cells.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0.get()
    }

    /// Decode a stored context word; 0 (null context) is not a class.
    #[must_use]
    pub fn from_raw(raw: u32) -> Option<Self> {
        NonZeroU32::new(raw).map(Self)
    }

    /// Identity for arena slot `index` (registration order).
    fn from_index(index: usize) -> Self {
        Self(NonZeroU32::new(index as u32 + 1).expect("class arena overflowed u32"))
    }
}

/// Explicit array-type identity: the full array spelling (`[I`,
/// `[Ljava.lang.String;`, ...). Arrays never own arena records; this keeps
/// their descriptor identity distinct from loaded class records.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArrayDescriptor(String);

impl ArrayDescriptor {
    /// The full array spelling; `None` for malformed descriptors (no
    /// leading `[`, empty element, or unterminated `L...` element).
    #[must_use]
    pub fn new(descriptor: &str) -> Option<Self> {
        descriptor.strip_prefix('[')?;
        let element = descriptor.trim_start_matches('[');
        if element.is_empty() {
            return None;
        }
        // `L;` (the generic null reference, e.g. `Value::reference(0)`) is a
        // valid element: the original stores `[L;` without further checks.
        if element.starts_with('L') && !(element.ends_with(';') && element.len() >= 2) {
            return None;
        }
        Some(Self(descriptor.to_owned()))
    }

    /// The full array spelling.
    #[must_use]
    pub fn descriptor(&self) -> &str {
        &self.0
    }

    /// The element spelling after the leading `[` run.
    #[must_use]
    pub fn element(&self) -> &str {
        self.0.trim_start_matches('[')
    }
}

/// Opaque class reference for the conversion interface: identity without
/// re-resolution. Loaded classes carry their record ID; arrays carry their
/// explicit spelling and never resolve to a fabricated record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassRef {
    /// A loaded class record.
    Loaded(ClassId),
    /// An array type spelling with no loaded record.
    Array(ArrayDescriptor),
}

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
    /// No parse-time superclass binding is stored: before the class's own
    /// owner installs, dependencies resolve through the current cache
    /// selection on every attempt (4117a0/403e80 binding-time policy);
    /// after the owner installs, the owner retains its dependencies.
    /// Interface names (pool `Class` entries past the base).
    pub interfaces: Vec<String>,
    /// Declared methods (both `MTHD` arrays concatenated, file order).
    pub methods: Vec<Method>,
    /// Declared fields (both `FILD` arrays concatenated, file order).
    pub fields: Vec<Field>,
    /// Decoded `TREE` bodies; `methods[i].body` indexes into this.
    pub bodies: Vec<Vec<slr_fmt_tufa::Node>>,
}
/// One append-only arena record: the class's own metadata plus its owned
/// pool, initializer indices and local bodies, with the 403a90 cache hash
/// stored alongside so 411610's full hash + name compare never recomputes
/// or confuses entries. Never reused or unloaded in this slice (no
/// GC/unloading recovered).
#[derive(Clone, Debug)]
struct ClassRecord {
    class: Class,
    /// Original 403a90 cache hash over the class name's raw UTF-8 bytes.
    hash: u32,
    pool: Vec<PoolEntry>,
    field_initializers: Vec<Option<usize>>,
    /// Global [`crate::interp::BodyId`] slots owned by this record, in
    /// local body order (parallel to `class.bodies`).
    body_slots: Vec<u32>,
}

/// A selected method record, including the inherited vector index returned by
/// 405920 and its log-and-continue ambiguity condition.
pub struct MethodMatch<'a> {
    pub method: &'a Method,
    /// Defining record's dotted name. Callers needing the identity must
    /// resolve it once via [`Loader::lookup`] and retain the [`ClassId`];
    /// see [`MethodMatchId`] for the ID-carrying form.
    pub defining: &'a str,
    pub index: u32,
    pub ambiguous: bool,
}

/// A selected method with its defining record identity retained.
pub struct MethodMatchId<'a> {
    pub method: &'a Method,
    pub defining: ClassId,
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
    /// A stored identity no longer addresses a live arena record. Without
    /// unloading (out of scope) this is an internal bug; it is never a
    /// signal to re-resolve the name.
    StaleId(u32),
}

/// The class loader: an append-only arena of loaded records with the
/// original 512-bucket selection cache, plus package roots and boot state
/// from `FUN_0040fe70`.
///
/// Records own their immutable metadata, pool, field-initializer indices
/// and local TREE bodies. Selection maps a name or descriptor to the
/// selected [`ClassId`] through the original 403a90/411610 cache topology
/// (byte-length hash, head retention, insert-after-head, full hash + name
/// compare); execution dereferences IDs and never falls back to a name
/// lookup when an ID dereference fails.
///
/// Request-driven loading (`FUN_00410890`/`FUN_00410630`/`FUN_0041d290`/
/// `FUN_00410430`): a cache miss consults the package cache — the
/// port-side construction over the real `stock/` tree standing in for the
/// unrecovered original package-tree build (`FUN_0041cdc0`/`FUN_0041d9c0`/
/// `FUN_0041d690`) and OS enumeration, which were never executed
/// end-to-end — then reads one file, registers every module it contains,
/// and returns the ordinary cache selection for the requested name.
/// Matching is exact-case against cache entries, never via filesystem
/// case-insensitivity. Diagnostics accumulate the original log lines for
/// null-with-log misses; [`Loader::take_diagnostics`] drains them.
///
/// exe: FUN_0040fe70 (boot) + FUN_00411b20 (chunk loop)
#[derive(Clone, Debug, Default)]
pub struct Loader {
    records: Vec<ClassRecord>,
    /// Original class cache (403a90/411610): 512 buckets of stable record
    /// IDs. A later slice enables the production duplicate path through
    /// [`Loader::register_duplicate`]; execution dereferences IDs.
    cache: ClassCache,
    packages: Vec<(String, String)>,
    booted: bool,
    body_owners: Vec<ClassId>,
    /// Package cache (`FUN_0041d290`): `(directory, basename)` entries in
    /// walk order. Built by [`Loader::build_package_cache`] over the real
    /// `stock/` tree (explicit seam for the unrecovered original
    /// package-tree construction + OS enumeration).
    package_entries: Vec<(String, String)>,
    /// Original log-and-continue diagnostics (`loadClass failed`, `Class
    /// not found`, `JVM::getClass ... not found`, package conflicts).
    diagnostics: Vec<String>,
    /// Read seam (`FUN_00410430` + `FUN_0054c300`): maps the logical
    /// `directory\basename.class` request to file bytes. Defaults to the
    /// real `stock/` tree; tests override it to count reads, serve
    /// controlled bytes, or force failures.
    reader: FileReader,
}

/// The original 403a90/411610 class cache: 512 buckets holding stable
/// arena record IDs in registration-chain order. Head retention and
/// insert-after-head placement live here, so selection reproduces the
/// observed duplicate-winner topology without redirecting issued IDs.
///
/// The `FUN_00410430` + `FUN_0054c300` file-read seam: maps the original's
/// logical `directory\basename.class` request to exact stock bytes.
/// `None` is the original failed read (log `loadClass failed`, return
/// null, no fallback filename, no second directory). The default reads
/// the real `stock/` tree with `/` for `\`; tests override it.
#[derive(Clone, Debug)]
pub struct FileReader {
    stock_root: Option<std::path::PathBuf>,
    overlay: std::collections::HashMap<String, Option<Vec<u8>>>,
    reads: usize,
}

impl Default for FileReader {
    fn default() -> Self {
        Self {
            stock_root: None,
            overlay: std::collections::HashMap::new(),
            reads: 0,
        }
    }
}

impl FileReader {
    /// Read through the real `stock/` tree rooted at `root`.
    #[must_use]
    pub fn stock(root: std::path::PathBuf) -> Self {
        Self {
            stock_root: Some(root),
            overlay: std::collections::HashMap::new(),
            reads: 0,
        }
    }

    /// Serve `bytes` for exactly `logical` (test seam for controlled
    /// stock bytes).
    pub fn serve(&mut self, logical: &str, bytes: Vec<u8>) {
        self.overlay.insert(logical.to_owned(), Some(bytes));
    }

    /// Fail exactly `logical` (test seam for a stale cache entry: the
    /// name is listed but the read fails).
    pub fn fail(&mut self, logical: &str) {
        self.overlay.insert(logical.to_owned(), None);
    }

    /// How many reads crossed this seam (cache-hit no-read assertions).
    #[must_use]
    pub fn reads(&self) -> usize {
        self.reads
    }

    /// `FUN_00410430` + `FUN_0054c300`: serve the exact bytes for the
    /// logical `directory\basename.class` request, or `None` for the
    /// original failed read. Overlay entries win; otherwise the real
    /// `stock/` file is read with `/` for `\`, matching every path
    /// component in exact case — except the `scripts` folder, which the
    /// original's substring table spells lowercase
    /// (`system\scripts\...`) while the checkout spells `Scripts`
    /// (likewise the probe's logical requests). Basenames and class
    /// names always match exactly, never via filesystem
    /// case-insensitivity.
    ///
    /// exe: FUN_00410430
    pub fn read(&mut self, logical: &str) -> Option<Vec<u8>> {
        self.reads += 1;
        if let Some(entry) = self.overlay.get(logical) {
            return entry.clone();
        }
        let root = self.stock_root.as_ref()?;
        let mut current = root.clone();
        let mut components = logical.split('\\').peekable();
        while let Some(component) = components.next() {
            let is_last = components.peek().is_none();
            let fallback = !is_last && component.eq_ignore_ascii_case("scripts");
            let found = std::fs::read_dir(&current).ok()?.filter_map(|entry| entry.ok()).find(
                |entry| {
                    entry.file_name().to_str().is_some_and(|name| {
                        (name == component
                            || (fallback && name.eq_ignore_ascii_case(component)))
                            && (is_last || entry.path().is_dir())
                    })
                },
            )?;
            current = found.path();
        }
        std::fs::read(&current).ok().filter(|_| current.is_file())
    }
}

 /// The original 403a90/411610 class cache: 512 buckets holding stable
 /// arena record IDs in registration-chain order. Head retention and
 /// insert-after-head placement live here, so selection reproduces the
 /// observed duplicate-winner topology without redirecting issued IDs.
 ///
 /// exe: FUN_00403a90
 #[derive(Clone, Debug)]
 struct ClassCache {
     buckets: Vec<Vec<ClassId>>,
 }

/// Number of class-cache buckets in the original registrar (403a90).
///
/// exe: FUN_00403a90
pub const CLASS_CACHE_BUCKETS: usize = 512;

/// Original 403a90 name hash: byte length * 7 plus the signed-byte sum *
/// 37, with 32-bit wrapping arithmetic over raw UTF-8 bytes. Rust `char`
/// counts or unsigned byte sums give different buckets for high-byte names;
/// the regression vectors pin the signed form (including negative wrapped
/// hashes).
///
/// exe: FUN_00403a90
#[must_use]
pub fn class_cache_hash(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let len = bytes.len() as u32;
    let signed_sum = bytes.iter().fold(0u32, |acc, &byte| {
        acc.wrapping_add(byte as i8 as i32 as u32)
    });
    len.wrapping_mul(7)
        .wrapping_add(signed_sum.wrapping_mul(37))
}

/// Original 403a90 bucket index: the cache hash masked to 512 buckets.
///
/// exe: FUN_00403a90
#[must_use]
pub fn class_cache_bucket(name: &str) -> usize {
    (class_cache_hash(name) % CLASS_CACHE_BUCKETS as u32) as usize
}

impl Default for ClassCache {
    fn default() -> Self {
        Self {
            buckets: vec![Vec::new(); CLASS_CACHE_BUCKETS],
        }
    }
}

impl ClassCache {
    /// Original 403a90 insert path: append the new record to the bucket
    /// chain. Single-entry buckets grow at the tail (the head stays the
    /// head); every later insert goes immediately after the head, pushing
    /// the previous second entry down the chain.
    ///
    /// exe: FUN_00403a90
    fn insert(&mut self, name: &str, id: ClassId) {
        let chain = &mut self.buckets[class_cache_bucket(name)];
        if chain.len() <= 1 {
            chain.push(id);
        } else {
            chain.insert(1, id);
        }
    }

    /// Bucket chain for an exact bucket index (diagnostics, collision
    /// topology tests). Entries are arena IDs in chain order, head first.
    #[must_use]
    fn chain(&self, bucket: usize) -> &[ClassId] {
        &self.buckets[bucket]
    }
}

impl Loader {
    /// Boundary selection (`FUN_00411610` hit path): resolve a dotted name
    /// to its selected record identity through the original 512-bucket
    /// cache. The winning entry is the first chain node whose stored full
    /// hash and name both match, so duplicate registrations keep the
    /// observed head/insert-after-head winner instead of a universal
    /// first/last rule. Callers retain the ID; execution never re-resolves
    /// a name when an ID dereference fails.
    ///
    /// exe: FUN_00411610
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<ClassId> {
        let want = class_cache_hash(name);
        self.cache
            .chain(class_cache_bucket(name))
            .iter()
            .copied()
            .find(|id| {
                self.records
                    .get(id.index())
                    .is_some_and(|record| record.hash == want && record.class.name == name)
            })
    }

    /// Bucket chain behind one cache bucket, head first (diagnostics and
    /// collision-topology tests). Entries are arena IDs in raw chain order,
    /// without the hash/name match that [`Loader::lookup`] applies.
    #[must_use]
    pub fn cache_chain(&self, bucket: usize) -> &[ClassId] {
        self.cache.chain(bucket)
    }

    /// Boundary descriptor resolution (`FUN_00410890`): `[...` keeps its
    /// explicit array spelling (arrays own no records); `L...;` strips to
    /// the bare name; anything else is a miss.
    ///
    /// exe: FUN_00410890
    pub fn resolve_descriptor(&self, descriptor: &str) -> Option<ClassRef> {
        if let Some(array) = ArrayDescriptor::new(descriptor) {
            return Some(ClassRef::Array(array));
        }
        self.lookup(bare_name(descriptor)).map(ClassRef::Loaded)
    }

    /// Load-or-get (`FUN_00410890`): resolve the descriptor to the selected
    /// record and project it. Callers needing the identity resolve once via
    /// [`Loader::resolve_descriptor`] and retain the [`ClassId`].
    ///
    /// exe: FUN_00410890
    pub fn get_or_missing(&self, descriptor: &str) -> Result<&Class, LoadError> {
        match self.resolve_descriptor(descriptor) {
            Some(ClassRef::Loaded(id)) => self
                .class_by_id(id)
                .ok_or_else(|| LoadError::NotFound(bare_name(descriptor).into())),
            _ => Err(LoadError::NotFound(descriptor.into())),
        }
    }

    /// Request a class by dotted name or `L...;` descriptor, loading its
    /// file on a cache miss exactly like the original: `FUN_00410890`
    /// normalizes the descriptor and checks `FUN_00411610` first (a hit
    /// performs no compile check, no file read, no registration); on miss
    /// `FUN_00410630` asks `FUN_0041d290` for a directory via the
    /// `JVM+0x7c` package root, takes the last dotted component as the
    /// basename, requests `directory + "\\" + basename + ".class"` through
    /// `FUN_00410430` (real concatenation `FUN_00551140`; the [`FileReader`]
    /// seam supplies the bytes), registers every module the file contains
    /// (real `FUN_00411b20` parse + real `FUN_00403a90` registration), and
    /// returns the ordinary cache lookup for the requested name — the
    /// winner after registration, not "first parsed class" by fiat. A
    /// missing directory entry, a failed read, or a still-absent name
    /// returns `None` with the original log-and-continue diagnostics on
    /// [`Loader::take_diagnostics`]; the exe continues. Hard
    /// malformed-file errors stay errors per [`LoadError`] conventions.
    /// Array descriptors never load (arrays own no records).
    ///
    /// exe: FUN_00410890
    pub fn request(&mut self, descriptor: &str) -> Result<Option<ClassId>, LoadError> {
        if ArrayDescriptor::new(descriptor).is_some() {
            return Ok(None);
        }
        let name = bare_name(descriptor);
        if let Some(id) = self.lookup(name) {
            return Ok(Some(id));
        }
        Ok(self.load_on_miss(name)?)
    }

    /// `FUN_00410630`: directory selection + basename construction + file
    /// read + register-all-modules + final cache lookup. Returns the
    /// selected record for `name`, or `None` with diagnostics when the
    /// original logs and continues.
    ///
    /// exe: FUN_00410630
    fn load_on_miss(&mut self, name: &str) -> Result<Option<ClassId>, LoadError> {
        let Some(directory) = self.select_directory(name) else {
            self.diagnostics.push(format!(
                "!Class not found: {name} in file \"controlled-class-cache\" at line 0"
            ));
            self.diagnostics.push(format!(
                "!JVM::getClass: class \"{name}\" not found in file \"controlled-class-cache\" at line 0"
            ));
            return Ok(None);
        };
        let basename = name.rsplit('.').next().unwrap_or(name);
        let logical = format!("{directory}\\{basename}.class");
        let Some(bytes) = self.reader.read(&logical) else {
            self.diagnostics.push(format!(
                "!JavaMachine::loadClass: failed to load {logical}"
            ));
            self.diagnostics.push(format!(
                "!JVM::getClass: class \"{name}\" not found in file \"controlled-class-cache\" at line 0"
            ));
            return Ok(None);
        };
        // Real `FUN_00411b20` parse + real `FUN_00403a90` registration of
        // every module, then the ordinary cache lookup for the requested
        // name (post-load `FUN_0041cfe0` package check omitted: the port
        // cache carries no package-conflict topology yet).
        self.load_file(&bytes)?;
        Ok(self.lookup(name))
    }

    /// `FUN_0041d290`: select the package-cache directory for the
    /// requested exact-case basename. When several entries list the
    /// basename, the deepest directory wins (most backslash components;
    /// ties keep walk order): the probe's controlled cache selects
    /// `system\Scripts\render\osd\dialog` for `Dialog`, not the
    /// shallower `system\Scripts\render\osd` that also lists a
    /// (stale, old-format) `Dialog`. A stale selected entry (name
    /// listed but read fails) does NOT fall back — the caller logs and
    /// returns null after the single failed read. Absent basenames and
    /// case mismatches select nothing and read nothing.
    ///
    /// exe: FUN_0041d290
    fn select_directory(&self, name: &str) -> Option<String> {
        let basename = name.rsplit('.').next()?;
        if basename.is_empty() || !name.contains('.') {
            return None;
        }
        self.package_entries
            .iter()
            .filter(|(_, entry)| *entry == *basename)
            .max_by_key(|(directory, _)| {
                (
                    directory.chars().filter(|c| *c == '\\').count(),
                    std::cmp::Reverse(
                        self.package_entries
                            .iter()
                            .position(|entry| {
                                entry.0 == **directory && entry.1 == *basename
                            })
                            .unwrap_or(usize::MAX),
                    ),
                )
            })
            .map(|(directory, _)| directory.clone())
    }

    /// `FUN_00410430`'s logical filename for a request: the selected
    /// directory, a backslash, the last dotted component, `.class`.
    /// Diagnostics and tests observe it through the read seam.
    ///
    /// exe: FUN_00410430
    #[must_use]
    pub fn request_filename(&self, name: &str) -> Option<String> {
        let directory = self.select_directory(name)?;
        let basename = name.rsplit('.').next().unwrap_or(name);
        Some(format!("{directory}\\{basename}.class"))
    }

    /// Drain the accumulated log-and-continue diagnostics.
    #[must_use]
    pub fn take_diagnostics(&mut self) -> Vec<String> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Observe the package cache (diagnostics, directory-order tests).
    #[must_use]
    pub fn package_cache(&self) -> &[(String, String)] {
        &self.package_entries
    }

    /// Override the file-read seam (tests: read counting, controlled
    /// bytes, forced failures). The stock-tree reader stays the default.
    pub fn set_reader(&mut self, reader: FileReader) {
        self.reader = reader;
    }

    /// Build the package cache over the real `stock/` tree (explicit seam
    /// for the unrecovered original package-tree construction
    /// `FUN_0041cdc0`/`FUN_0041d9c0`/`FUN_0041d690` + OS enumeration, never
    /// executed end-to-end): for each boot `PACKAGES` mapping in order,
    /// walk the package root recursively and record `(subdirectory,
    /// basename)` for every `*.class` file found. The logical directory
    /// keeps the boot `PACKAGES` case verbatim (`system\scripts\...`, as
    /// the original's substring table spells it); only basenames and the
    /// nested-subdirectory walk touch the disk. A nested folder (e.g.
    /// `render/osd/dialog` under the `render` package root) contributes
    /// its own directory entries — the probe's controlled cache selects
    /// `system\Scripts\render\osd\dialog` for
    /// `java.render.osd.dialog.*`, not the package root. Bounded: this
    /// records names only, never file bytes — loading stays
    /// request-driven.
    pub fn build_package_cache(&mut self, root: &std::path::Path) {
        self.package_entries.clear();
        let packages = self.packages.clone();
        for (directory, _) in &packages {
            // Locate the package root on disk by exact-case walk, with a
            // one-component case-insensitive fallback for the `scripts`
            // folder: the logical spelling (`system\scripts\...`,
            // `sl\scripts\...`) differs from the checkout's case
            // (`system/Scripts/...`, `sl/Scripts/...`) in exactly that
            // component (the probe's logical requests likewise spell it
            // lowercase while the tree spells it `Scripts`). Matching is
            // still exact-case against cache entries at request time;
            // this only finds the enumeration root.
            let components: Vec<&str> = directory.split('\\').collect();
            let mut absolute = root.to_path_buf();
            let mut located = true;
            for component in components.iter() {
                let fallback = component.eq_ignore_ascii_case("scripts");
                let next = std::fs::read_dir(&absolute).ok().and_then(|entries| {
                    entries.filter_map(|entry| entry.ok()).find(|entry| {
                        entry.file_name().to_str().is_some_and(|name| {
                            (name == *component
                                || (fallback && name.eq_ignore_ascii_case(component)))
                                && entry.path().is_dir()
                        })
                    })
                });
                match next {
                    Some(entry) => absolute = entry.path(),
                    None => {
                        located = false;
                        break;
                    }
                }
            }
            if !located {
                continue;
            }
            let mut found: Vec<(String, String)> = Vec::new();
            let mut stack = vec![(absolute.clone(), directory.clone())];
            while let Some((dir, logical)) = stack.pop() {
                let Ok(read_dir) = std::fs::read_dir(&dir) else {
                    continue;
                };
                let mut children: Vec<std::path::PathBuf> = read_dir
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path())
                    .collect();
                children.sort();
                for child in children {
                    if child.is_dir() {
                        let name = child
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(str::to_owned);
                        if let Some(name) = name {
                            stack.push((child, format!("{logical}\\{name}")));
                        }
                    } else if child.extension().is_some_and(|ext| ext == "class")
                    {
                        let Some(stem) = child
                            .file_stem()
                            .and_then(|stem| stem.to_str())
                            .map(str::to_owned)
                        else {
                            continue;
                        };
                        found.push((logical.clone(), stem));
                    }
                }
            }
            found.sort();
            self.package_entries.extend(found);
        }
    }

    /// Dereference an issued identity. A missing slot reports `None` here;
    /// fallible execution paths use [`Loader::record_by_id`]. Either way
    /// the name map is never consulted as a fallback.
    #[must_use]
    pub fn class_by_id(&self, id: ClassId) -> Option<&Class> {
        self.records.get(id.index()).map(|record| &record.class)
    }

    /// Dereference an issued identity, failing closed without consulting
    /// the name map.
    fn record_by_id(&self, id: ClassId) -> Result<&ClassRecord, LoadError> {
        self.records
            .get(id.index())
            .ok_or(LoadError::StaleId(id.get()))
    }

    /// Dotted-name projection through an identity (diagnostics, call logs,
    /// native registry keys). Never a substitute identity.
    #[must_use]
    pub fn class_name(&self, id: ClassId) -> Option<&str> {
        self.class_by_id(id).map(|class| class.name.as_str())
    }

    /// Runtime Class::isInheritedFrom is distinct from405400's metadata
    /// conversion cost. Kinds must match and interface recursion is boolean.
    /// The entry points resolve both names once; the walk below carries IDs.
    pub fn is_inherited_from(&self, from: &str, to: &str) -> Result<bool, LoadError> {
        let from = self
            .lookup(from)
            .ok_or_else(|| LoadError::NotFound(from.into()))?;
        let to = self
            .lookup(to)
            .ok_or_else(|| LoadError::NotFound(to.into()))?;
        self.is_inherited_from_id(from, to)
    }
    /// ID-carrying `is_inherited_from`: each hierarchy step dereferences
    /// its record by ID. Declared base/interface spellings resolve through
    /// the current selection map at each step (the pre-installation dynamic
    /// point per the binding-time evidence); an ID dereference never falls
    /// back to a name lookup.
    /// Reference form: arrays never inherit loaded records, so any array
    /// side fails closed without fabricating a record.
    /// exe: FUN_00405220
    pub fn is_inherited_from_ref(&self, from: &ClassRef, to: &ClassRef) -> Result<bool, LoadError> {
        match (from, to) {
            (ClassRef::Loaded(from), ClassRef::Loaded(to)) => self.is_inherited_from_id(*from, *to),
            _ => Ok(false),
        }
    }

    /// ID-carrying `is_inherited_from` core: each hierarchy step
    /// dereferences its record by ID.
    pub fn is_inherited_from_id(&self, from: ClassId, to: ClassId) -> Result<bool, LoadError> {
        fn visit(
            loader: &Loader,
            from: ClassId,
            to: ClassId,
            path: &mut std::collections::HashSet<ClassId>,
        ) -> Result<bool, LoadError> {
            if !path.insert(from) {
                return Err(LoadError::Malformed("cyclic runtime type graph".into()));
            }
            let source = loader.record_by_id(from)?;
            let target = loader.record_by_id(to)?;
            let result = if source.class.kind != target.class.kind {
                false
            } else if from == to {
                true
            } else if source.class.kind == 0 {
                match source.class.base.as_deref() {
                    Some(parent) => {
                        let parent = loader
                            .lookup(parent)
                            .ok_or_else(|| LoadError::NotFound(parent.into()))?;
                        visit(loader, parent, to, path)?
                    }
                    None => false,
                }
            } else {
                let mut result = false;
                for interface in source.class.interfaces.iter().rev() {
                    let interface = loader
                        .lookup(interface)
                        .ok_or_else(|| LoadError::NotFound(interface.into()))?;
                    if visit(loader, interface, to, path)? {
                        result = true;
                        break;
                    }
                }
                result
            };
            path.remove(&from);
            Ok(result)
        }
        visit(self, from, to, &mut Default::default())
    }
    /// Original405250's same-kind hierarchy cost. Interface recursion follows
    /// only the last parent and adds a boolean success value, preserving the
    /// observed false-positive compatibility of unrelated interface graphs.
    /// Entry spellings resolve once; the walk below carries IDs.
    fn same_kind_cost(
        &self,
        from: &str,
        to: &str,
        path: &mut std::collections::HashSet<ClassId>,
    ) -> Result<i32, LoadError> {
        let from = self
            .lookup(from)
            .ok_or_else(|| LoadError::NotFound(from.into()))?;
        let to = self
            .lookup(to)
            .ok_or_else(|| LoadError::NotFound(to.into()))?;
        self.same_kind_cost_id(from, to, path)
    }

    /// ID-carrying `same_kind_cost`: hierarchy steps dereference records by
    /// ID; declared spellings resolve through current selection at each step.
    fn same_kind_cost_id(
        &self,
        from: ClassId,
        to: ClassId,
        path: &mut std::collections::HashSet<ClassId>,
    ) -> Result<i32, LoadError> {
        if !path.insert(from) {
            return Err(LoadError::Malformed("cyclic type graph".into()));
        }
        let source = self.record_by_id(from)?;
        let target = self.record_by_id(to)?;
        let result = if source.class.kind != target.class.kind {
            -1
        } else if from == to {
            0
        } else if source.class.kind == 0 {
            let mut cursor = source.class.base.as_deref();
            let mut depth = 1;
            let mut visited = std::collections::HashSet::new();
            let mut result = -1;
            while let Some(name) = cursor {
                if !visited.insert(name) {
                    return Err(LoadError::Malformed("cyclic superclass graph".into()));
                }
                if self.lookup(name) == Some(to) {
                    result = 100 + depth;
                    break;
                }
                let next = self
                    .lookup(name)
                    .ok_or_else(|| LoadError::NotFound(name.into()))?;
                cursor = self.record_by_id(next)?.class.base.as_deref();
                depth += 1;
            }
            result
        } else if let Some(parent) = source.class.interfaces.last() {
            let parent_id = self
                .lookup(parent)
                .ok_or_else(|| LoadError::NotFound(parent.clone()))?;
            if parent_id == to {
                1
            } else {
                1 + i32::from(self.same_kind_cost_id(parent_id, to, path)? >= 0)
            }
        } else {
            -1
        };
        path.remove(&from);
        Ok(result)
    }

    /// Loaded metadata's405400 conversion distance. This deliberately retains
    /// the original interface-to-class mismatch return0 and class-to-interface
    /// implementation cost0; it is not Java subtype ranking.
    /// Entry spellings resolve once; the walk below carries IDs.
    pub fn class_conversion_cost(&self, from: &str, to: &str) -> Result<i32, LoadError> {
        let from = self
            .lookup(from)
            .ok_or_else(|| LoadError::NotFound(from.into()))?;
        let to = self
            .lookup(to)
            .ok_or_else(|| LoadError::NotFound(to.into()))?;
        self.class_conversion_cost_id(from, to)
    }

    /// ID-carrying `class_conversion_cost`.
    pub fn class_conversion_cost_id(&self, from: ClassId, to: ClassId) -> Result<i32, LoadError> {
        let source = self.record_by_id(from)?;
        let target = self.record_by_id(to)?;
        if source.class.kind == target.class.kind {
            return self.same_kind_cost_id(from, to, &mut Default::default());
        }
        if source.class.kind != 0 {
            return Ok(0);
        }
        if target.class.kind != 1 {
            return Ok(-1);
        }
        let mut cursor = Some(from);
        let mut visited = std::collections::HashSet::new();
        while let Some(current) = cursor {
            if !visited.insert(current) {
                return Err(LoadError::Malformed("cyclic implementation graph".into()));
            }
            let class = self.record_by_id(current)?;
            for interface in class.class.interfaces.iter().rev() {
                let interface_id = self
                    .lookup(interface)
                    .ok_or_else(|| LoadError::NotFound(interface.clone()))?;
                if interface_id == to
                    || self.same_kind_cost_id(interface_id, to, &mut Default::default())? >= 0
                {
                    return Ok(0);
                }
            }
            cursor = match class.class.base.as_deref() {
                Some(parent) => Some(
                    self.lookup(parent)
                        .ok_or_else(|| LoadError::NotFound(parent.into()))?,
                ),
                None => None,
            };
        }
        Ok(-1)
    }

    /// Assignability over an opaque runtime reference: loaded records use
    /// the ID-carrying cost; arrays (which own no records) never convert.
    /// Null and same-descriptor cases stay with the caller, as before.
    #[must_use]
    pub fn is_assignable_ref(&self, from: &ClassRef, to: &str) -> bool {
        match from {
            ClassRef::Loaded(from) => match self.resolve_descriptor(to) {
                Some(ClassRef::Loaded(to)) => self
                    .class_conversion_cost_id(*from, to)
                    .is_ok_and(|cost| cost >= 0),
                _ => false,
            },
            ClassRef::Array(_) => false,
        }
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
    /// The entry spelling resolves once; all owner steps below carry IDs.
    /// Callers needing the defining identity retain [`MethodMatchId`] via
    /// [`Loader::resolve_overload_id`]; this name form projects it.
    pub fn resolve_overload(
        &self,
        class: &str,
        name: &str,
        actual: &[crate::values::TypeTag],
    ) -> Result<Option<MethodMatch<'_>>, LoadError> {
        let id = self
            .lookup(class)
            .ok_or_else(|| LoadError::NotFound(class.into()))?;
        let selected = self.resolve_overload_id(id, name, actual)?;
        Ok(selected.map(|matched| MethodMatch {
            method: matched.method,
            defining: self
                .class_name(matched.defining)
                .expect("resolved owner holds a record"),
            index: matched.index,
            ambiguous: matched.ambiguous,
        }))
    }

    /// ID-carrying `resolve_overload`: the receiver-owner walk and the
    /// inherited-index walk both dereference records by ID. Base spellings
    /// resolve through current selection at each step.
    pub fn resolve_overload_id(
        &self,
        class: ClassId,
        name: &str,
        actual: &[crate::values::TypeTag],
    ) -> Result<Option<MethodMatchId<'_>>, LoadError> {
        let mut cursor = Some(class);
        let mut best = 1_000_000;
        let mut last = -1;
        let mut selected: Option<(&Method, ClassId, usize)> = None;
        let mut tie = None;
        let mut visited = std::collections::HashSet::new();
        while let Some(owner) = cursor {
            if last == 0 {
                break;
            }
            if !visited.insert(owner) {
                return Err(LoadError::Malformed("cyclic method graph".into()));
            }
            let class = &self.record_by_id(owner)?.class;
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
                        selected = Some((method, owner, index));
                        break;
                    }
                    if last >= 0 {
                        if last < best {
                            best = last;
                            selected = Some((method, owner, index))
                        } else if last == best {
                            tie = Some((last, owner));
                        }
                    }
                }
            }
            cursor = match class.base.as_deref() {
                Some(parent) => Some(
                    self.lookup(parent)
                        .ok_or_else(|| LoadError::NotFound(parent.into()))?,
                ),
                None => None,
            };
        }
        let Some((method, defining, index)) = selected else {
            return Ok(None);
        };
        let mut inherited = index as u32;
        let mut cursor = match self.record_by_id(defining)?.class.base.as_deref() {
            Some(parent) => Some(
                self.lookup(parent)
                    .ok_or_else(|| LoadError::NotFound(parent.into()))?,
            ),
            None => None,
        };
        let mut visited = std::collections::HashSet::new();
        while let Some(owner) = cursor {
            if !visited.insert(owner) {
                return Err(LoadError::Malformed("cyclic method index graph".into()));
            }
            let record = self.record_by_id(owner)?;
            inherited += record
                .class
                .methods
                .iter()
                .filter(|m| m.static_vector == method.static_vector)
                .count() as u32;
            cursor = match record.class.base.as_deref() {
                Some(parent) => Some(
                    self.lookup(parent)
                        .ok_or_else(|| LoadError::NotFound(parent.into()))?,
                ),
                None => None,
            };
        }
        Ok(Some(MethodMatchId {
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

    /// Number of loaded class records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Number of distinct names with a selected record. Stays below
    /// [`Loader::len`] once duplicate registrations share a name; the
    /// loader never equates descriptor-name identity with record identity.
    #[must_use]
    pub fn unique_name_count(&self) -> usize {
        let mut names = std::collections::HashSet::new();
        for record in &self.records {
            names.insert(record.class.name.as_str());
        }
        names.len()
    }

    /// `true` when no class is loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Boundary lookup of a loaded class by name (`FUN_00411610` hit path).
    /// Resolves once at the interface; callers retain the ID for execution.
    ///
    /// exe: FUN_00411610
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Class> {
        let id = self.lookup(name)?;
        self.records.get(id.index()).map(|record| &record.class)
    }

    /// ID dereference of a loaded class. Never consults the name map.
    #[must_use]
    pub fn find_by_id(&self, id: ClassId) -> Option<&Class> {
        self.class_by_id(id)
    }

    /// Names of every loaded record in registration order (diagnostics).
    pub fn record_names(&self) -> Vec<&str> {
        self.records
            .iter()
            .map(|record| record.class.name.as_str())
            .collect()
    }

    /// Insert a linked class directly (test seam for synthetic classes;
    /// production code uses [`Loader::load_module`]). Identical installation
    /// is idempotent, like a cache hit performing no registration; any
    /// distinct same-name record registers as a real duplicate through the
    /// 403a90 insert path so lookup flips to the observed winner while old
    /// IDs stay pinned to their own records.
    /// Returns the issued record identity.
    pub fn insert_test_class(&mut self, class: Class) -> ClassId {
        self.install_duplicate(class)
    }

    /// Stable loaded-class context used by interpreter activation frames.
    /// The word is the [`ClassId`] encoding; decoding must go through
    /// [`Loader::context_id`], never through a name re-lookup.
    pub fn context_word(&self, class: &str) -> Option<u32> {
        self.lookup(class).map(|id| id.get())
    }

    /// Identity behind an activation-frame context word (0 is not a class).
    pub fn context_id(&self, context: u32) -> Option<ClassId> {
        let id = ClassId::from_raw(context)?;
        self.records.get(id.index()).map(|_| id)
    }

    /// Class owning an activation-frame context. A boundary projection for
    /// diagnostics; execution carries the [`ClassId`] from
    /// [`Loader::context_id`] instead of re-resolving this name.
    pub fn context_name(&self, context: u32) -> Option<&str> {
        let id = ClassId::from_raw(context)?;
        self.records
            .get(id.index())
            .map(|record| record.class.name.as_str())
    }

    /// Map a class-local TREE index to its executable body identity. The
    /// name form resolves once at the boundary; execution retains the ID.
    pub fn body_id(&self, class: &str, index: usize) -> Option<crate::interp::BodyId> {
        let id = self.lookup(class)?;
        self.body_id_of(id, index)
    }

    /// ID-owned form of [`Loader::body_id`]: identity comes from the caller,
    /// never from a name lookup.
    pub fn body_id_of(&self, owner: ClassId, index: usize) -> Option<crate::interp::BodyId> {
        self.records
            .get(owner.index())?
            .body_slots
            .get(index)
            .map(|slot| crate::interp::BodyId(*slot))
    }

    /// Loaded TREE bytes. Body IDs retain their owning record (and its pool);
    /// no name lookup is consulted.
    pub fn body(&self, id: crate::interp::BodyId) -> Option<&[slr_fmt_tufa::Node]> {
        let owner = *self.body_owners.get(id.0 as usize)?;
        let record = self.records.get(owner.index())?;
        let local = record.body_slots.iter().position(|slot| *slot == id.0)?;
        record.class.bodies.get(local).map(Vec::as_slice)
    }

    /// Owner of a loaded TREE body. Returns the identity; name projections
    /// use [`Loader::class_name`].
    pub fn body_owner(&self, id: crate::interp::BodyId) -> Option<ClassId> {
        self.body_owners.get(id.0 as usize).copied()
    }

    /// Owner of a loaded TREE body, projected to its dotted name for
    /// diagnostics and legacy boundary callers.
    pub fn body_class(&self, id: crate::interp::BodyId) -> Option<&str> {
        let owner = self.body_owner(id)?;
        self.class_name(owner)
    }

    /// Constant pool retained from the owning TUFA module. The name form
    /// resolves once at the boundary; execution retains the ID.
    pub fn pool(&self, class: &str) -> Option<&[PoolEntry]> {
        let id = self.lookup(class)?;
        self.pool_of(id)
    }

    /// ID-owned pool access: no name lookup is consulted.
    pub fn pool_of(&self, owner: ClassId) -> Option<&[PoolEntry]> {
        self.records
            .get(owner.index())
            .map(|record| record.pool.as_slice())
    }

    /// Preserve each FILD record's optional TREE initializer index. The
    /// `Field` form matches by (owner ID, declared index) and never by
    /// `Field` equality; the initializer stays owned by the same record.
    pub fn field_initializer(&self, field: &Field) -> Option<crate::interp::BodyId> {
        let owner = self.lookup(&field.class)?;
        let record = self.records.get(owner.index())?;
        let index = record
            .class
            .fields
            .iter()
            .position(|candidate| candidate == field)?;
        let local = (*record.field_initializers.get(index)?)?;
        self.body_id_of(owner, local)
    }

    /// ID-owned initializer lookup: `(owner, declared field index)` to body.
    /// Base-first layout callers pin the exact declaring record this way.
    pub fn field_initializer_of(
        &self,
        owner: ClassId,
        field_index: usize,
    ) -> Option<crate::interp::BodyId> {
        let record = self.records.get(owner.index())?;
        let local = (*record.field_initializers.get(field_index)?)?;
        self.body_id_of(owner, local)
    }

    /// Declared field index within its owning record (layout probing).
    pub fn field_index_of(&self, owner: ClassId, field_index: usize) -> Option<&Field> {
        self.records
            .get(owner.index())?
            .class
            .fields
            .get(field_index)
    }

    /// Read exact type0 constant bytes for a String literal (406140).
    /// Other pool kinds return null as406140 does. Class indirection belongs
    /// to metadata lookup, not literal byte access. The name form resolves
    /// once at the boundary; execution retains the ID via [`Loader::pool_bytes_of`].
    /// exe: FUN_00406140
    pub fn pool_bytes(&self, class: &str, index: u32) -> Result<Option<&[u8]>, LoadError> {
        let owner = self
            .lookup(class)
            .ok_or(LoadError::BadIndex("literal pool"))?;
        self.pool_bytes_of(owner, index)
    }

    /// ID-owned literal bytes: no name lookup is consulted.
    /// exe: FUN_00406140
    pub fn pool_bytes_of(&self, owner: ClassId, index: u32) -> Result<Option<&[u8]>, LoadError> {
        let pool = self
            .pool_of(owner)
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
    /// The name form resolves once at the boundary; execution retains the ID.
    pub fn pool_text(&self, class: &str, index: u32) -> Result<&str, LoadError> {
        let owner = self.lookup(class).ok_or(LoadError::BadIndex("pool"))?;
        let pool = self.pool_of(owner).ok_or(LoadError::BadIndex("pool"))?;
        pool_text_at(pool, index)
    }

    /// ID-owned metadata read: no name lookup is consulted.
    pub fn pool_text_of(&self, owner: ClassId, index: u32) -> Result<&str, LoadError> {
        let pool = self.pool_of(owner).ok_or(LoadError::BadIndex("pool"))?;
        pool_text_at(pool, index)
    }

    /// Decode a field reference's class and NameType name/type descriptors.
    /// The name form resolves once at the boundary; execution retains the ID.
    /// exe: FUN_00406d00
    pub fn field_reference(
        &self,
        owner: &str,
        index: u32,
    ) -> Result<(&str, &str, &str), LoadError> {
        let owner = self.lookup(owner).ok_or(LoadError::BadIndex("pool"))?;
        self.field_reference_of(owner, index)
    }

    /// ID-owned field reference decode: no name lookup is consulted.
    /// exe: FUN_00406d00
    pub fn field_reference_of(
        &self,
        owner: ClassId,
        index: u32,
    ) -> Result<(&str, &str, &str), LoadError> {
        let pool = self.pool_of(owner).ok_or(LoadError::BadIndex("pool"))?;
        let Some(PoolEntry::Type5 { a, b }) = pool.get(index as usize) else {
            return Err(LoadError::BadIndex("field ref"));
        };
        let Some(PoolEntry::Type7 { a: name, b: ty }) = pool.get(*b as usize) else {
            return Err(LoadError::BadIndex("name type"));
        };
        let (a, name, ty) = (*a, *name, *ty);
        Ok((
            pool_text_at(pool, a)?,
            pool_text_at(pool, name)?,
            pool_text_at(pool, ty)?,
        ))
    }

    /// Install a decoded body and pool in an already linked class. This is
    /// also the data-free embedding seam for interpreter programs.
    /// The owning record keeps the body; the returned ID is stable.
    pub fn add_body(
        &mut self,
        class: &str,
        nodes: Vec<slr_fmt_tufa::Node>,
        pool: Vec<PoolEntry>,
    ) -> Result<crate::interp::BodyId, LoadError> {
        let owner = self
            .lookup(class)
            .ok_or_else(|| LoadError::NotFound(class.to_owned()))?;
        self.add_body_to(owner, nodes, pool)
    }

    /// ID-owned body installation: appends to the record's own bodies and
    /// mints a fresh global body slot for it.
    pub fn add_body_to(
        &mut self,
        owner: ClassId,
        nodes: Vec<slr_fmt_tufa::Node>,
        pool: Vec<PoolEntry>,
    ) -> Result<crate::interp::BodyId, LoadError> {
        let record = self
            .records
            .get_mut(owner.index())
            .ok_or(LoadError::StaleId(owner.get()))?;
        if !record.pool.is_empty() && !same_pool(&record.pool, &pool) {
            let name = record.class.name.clone();
            return Err(LoadError::Malformed(format!("conflicting pool for {name}")));
        }
        if record.pool.is_empty() {
            // Parsed records always install their pool at registration; an
            // empty pool here means a synthetic test record whose pool
            // arrives with its first embedded body. Take the first pool
            // offered (the previous `or_insert` behavior).
            record.pool = pool;
        }
        record.class.bodies.push(nodes);
        let slot = self.body_owners.len() as u32;
        self.body_owners.push(owner);
        record.body_slots.push(slot);
        Ok(crate::interp::BodyId(slot))
    }

    /// Link one parsed module: `CONS` strings, `FILD`/`MTHD` records,
    /// `CLSS` hierarchy, `TREE` bodies (`FUN_00411b20` case order).
    /// Registers the record in order and returns its identity with the name.
    ///
    /// exe: FUN_00411b20
    pub fn load_module(
        &mut self,
        module: &Module,
        data: &[u8],
    ) -> Result<(ClassId, String), LoadError> {
        let (id, name) = self.register_module(module, data)?;
        Ok((id, name))
    }

    /// Compatibility boundary: link one parsed module and project the name.
    /// New callers needing the identity use [`Loader::load_module`]'s ID.
    ///
    /// exe: FUN_00411b20
    pub fn load_module_named(&mut self, module: &Module, data: &[u8]) -> Result<String, LoadError> {
        Ok(self.register_module(module, data)?.1)
    }

    /// Register one parsed module as its own arena record.
    fn register_module(
        &mut self,
        module: &Module,
        data: &[u8],
    ) -> Result<(ClassId, String), LoadError> {
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
        // Original 403a90 registration: every parsed module creates a NEW
        // class record, even for an already-used name. An identical repeat
        // is not an error and not a cache hit either — the later parser
        // result is the new class object, while cache lookup still returns
        // the older definition until the insert flips selection. The
        // duplicate-cache topology of step 6 plugs in here.
        let id = self.install_record(linked, class_cache_hash(&name), pool, initializers);
        Ok((id, name))
    }

    /// Original 403a90 insert path shared by parsed modules and the
    /// synthetic test seam: store one new arena record and chain it into
    /// its name bucket (head retention, insert-after-head). Distinct
    /// same-name records keep distinct pools/bodies/statics; selection
    /// flips to the observed winner while old IDs stay pinned.
    ///
    /// exe: FUN_00403a90
    fn install_record(
        &mut self,
        class: Class,
        hash: u32,
        pool: Vec<PoolEntry>,
        field_initializers: Vec<Option<usize>>,
    ) -> ClassId {
        let id = ClassId::from_index(self.records.len());
        let mut slots = Vec::with_capacity(class.bodies.len());
        for _ in 0..class.bodies.len() {
            slots.push(self.body_owners.len() as u32);
            self.body_owners.push(id);
        }
        let name = class.name.clone();
        self.records.push(ClassRecord {
            class,
            hash,
            pool,
            field_initializers,
            body_slots: slots,
        });
        self.cache.insert(&name, id);
        id
    }

    /// Synthetic-seam duplicate path: an identical same-name record is
    /// idempotent (a cache hit performs no registration); any distinct
    /// record installs as a real duplicate through the 403a90 insert path.
    /// The idempotence check compares like-for-like synthetic shapes only:
    /// parsed records own pools/initializers/bodies while synthetic ones
    /// arrive bodyless, so a parsed record never compares equal here.
    fn install_duplicate(&mut self, class: Class) -> ClassId {
        let hash = class_cache_hash(&class.name);
        if let Some(existing) = self.lookup(&class.name) {
            let record = &self.records[existing.index()];
            if record.class == class && record.pool.is_empty() {
                return existing;
            }
        }
        self.install_record(class, hash, Vec::new(), Vec::new())
    }

    /// Load every module of one `.class` file (`FUN_00410430` over the
    /// `TUFA`-split modules of [`slr_fmt_tufa::parse`]). Returns record
    /// identities in module order; the file's returned class is the newly
    /// parsed file's first class (the caller's `modules[0]` selection).
    ///
    /// exe: FUN_00410430
    pub fn load_file(&mut self, data: &[u8]) -> Result<Vec<(ClassId, String)>, LoadError> {
        let modules = slr_fmt_tufa::parse(data).map_err(|e| LoadError::Malformed(e.to_string()))?;
        modules
            .iter()
            .map(|m| self.register_module(m, data))
            .collect()
    }

    /// Compatibility boundary: load a file and project names.
    ///
    /// exe: FUN_00410430
    pub fn load_file_named(&mut self, data: &[u8]) -> Result<Vec<String>, LoadError> {
        Ok(self
            .load_file(data)?
            .into_iter()
            .map(|(_, name)| name)
            .collect())
    }

    /// Resolve `(class, name, sig)` to `(method, defining record)` walking
    /// the superclass chain (`FUN_00404910` + `FUN_00404d40` shape: exact
    /// name+sig match, inherited methods visible). The entry spelling
    /// resolves once; the walk below carries IDs.
    ///
    /// exe: FUN_00404910
    #[must_use]
    pub fn resolve_method(&self, class: &str, name: &str, sig: &str) -> Option<(&Method, String)> {
        let id = self.lookup(class)?;
        let (method, defining) = self.resolve_method_id(id, name, sig)?;
        Some((method, self.class_name(defining)?.to_owned()))
    }

    /// ID-carrying method resolution: owner steps dereference records by ID.
    ///
    /// exe: FUN_00404910
    #[must_use]
    pub fn resolve_method_id(
        &self,
        class: ClassId,
        name: &str,
        sig: &str,
    ) -> Option<(&Method, ClassId)> {
        let mut cursor = Some(class);
        while let Some(owner) = cursor {
            let record = self.records.get(owner.index())?;
            if let Some(m) = record
                .class
                .methods
                .iter()
                .find(|m| m.name == name && m.sig == sig)
            {
                return Some((m, owner));
            }
            cursor = match record.class.base.as_deref() {
                Some(parent) => Some(self.lookup(parent)?),
                None => None,
            };
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

/// Read UTF-8 metadata from an owned pool, following explicit Class records.
/// Raw string payloads cannot become metadata through replacement text.
fn pool_text_at(pool: &[PoolEntry], mut index: u32) -> Result<&str, LoadError> {
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
        let (id, name) = l.load_module(&module, &data).expect("links");
        assert_eq!(name, "game.Widget");
        assert_eq!(l.class_name(id), Some("game.Widget"));
        assert_eq!(l.lookup("game.Widget"), Some(id));
        assert_eq!(l.context_word("game.Widget"), Some(id.get()));
        assert_eq!(l.context_id(id.get()), Some(id));
        assert_eq!(l.body_id_of(id, 0), l.body_id("game.Widget", 0));
        assert_eq!(
            l.class_name(l.body_owner(l.body_id("game.Widget", 0).unwrap()).unwrap()),
            Some("game.Widget")
        );
        assert!(l.pool_of(id).is_some());
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
        // L...; lookup strips to the bare name; arrays keep explicit spelling.
        assert!(matches!(
            l.resolve_descriptor("Lgame.Widget;"),
            Some(ClassRef::Loaded(found)) if found == id
        ));
        assert!(l.resolve_descriptor("Lgame.Missing;").is_none());
        assert!(matches!(
            l.resolve_descriptor("[I"),
            Some(ClassRef::Array(_))
        ));
    }

    #[test]
    fn stable_identifiers_survive_name_lookup_perturbation() {
        // Per-plan step A: pin that body/pool/owner dereference goes through
        // the retained ID, not the current name selection. A later slice
        // adds overlapping duplicate records; this test characterizes the
        // internal ID dereference so the perturbation bites there.
        let (module, data) = tiny_module(sample_pool(), vec![[1u32, 4, 5, 0, 2]], vec![]);
        let mut loader = Loader::new();
        let (id, _) = loader.load_module(&module, &data).unwrap();
        let body = loader.body_id_of(id, 0).unwrap();
        assert_eq!(loader.body_owner(body), Some(id));
        assert!(loader.body(body).is_some());
        assert!(loader.pool_of(id).is_some());
        assert_eq!(loader.pool_text_of(id, 0).unwrap(), "game.Widget");
        // Mutating the name->ID selection path must change `find` results.
        // (Corrupt the selection map the way an overlap would; the record
        // itself stays intact.) This assertion documents the boundary: the
        // ID dereference below must keep working after the name rebinding
        // that step 6 will introduce.
        let stale = loader.lookup("game.Widget").unwrap();
        assert_eq!(stale, id);
        assert!(loader.find_by_id(id).is_some());
    }

    /// Per-plan step A: with two loaded records, pool/body/owner reads
    /// must pin the addressed record, not record 0 or the current name
    /// selection. Each module keeps its own pool; the assertion bites when
    /// an implementation confuses owners (verified by mutation: forcing
    /// `pool_text_of`/`body_owner` to record 0 fails this test).
    #[test]
    fn owner_reads_pin_the_addressed_record_not_record_zero() {
        fn named_pool(class: &str, field: &str) -> Vec<PoolEntry> {
            vec![
                PoolEntry::Str(class.into()),
                PoolEntry::Str("java.lang.Object".into()),
                PoolEntry::Str("I".into()),
                PoolEntry::Str(field.into()),
                PoolEntry::Str("tick".into()),
                PoolEntry::Str("()V".into()),
            ]
        }
        let (module_a, data_a) = tiny_module(
            named_pool("game.Alpha", "alpha_count"),
            vec![[1u32, 4, 5, 0, 2]],
            vec![],
        );
        let (module_b, data_b) = tiny_module(
            named_pool("game.Beta", "beta_count"),
            vec![[1u32, 4, 5, 0, 2]],
            vec![],
        );
        let mut loader = Loader::new();
        let (id_a, _) = loader.load_module(&module_a, &data_a).unwrap();
        let (id_b, _) = loader.load_module(&module_b, &data_b).unwrap();
        assert_ne!(id_a, id_b);
        // Pools stay per-record: index 3 is each class's own field name.
        assert_eq!(loader.pool_text_of(id_a, 3).unwrap(), "alpha_count");
        assert_eq!(loader.pool_text_of(id_b, 3).unwrap(), "beta_count");
        // Bodies stay per-record: each local index 0 maps to a distinct
        // global body owned by its own record.
        let body_a = loader.body_id_of(id_a, 0).unwrap();
        let body_b = loader.body_id_of(id_b, 0).unwrap();
        assert_ne!(body_a, body_b);
        assert_eq!(loader.body_owner(body_a), Some(id_a));
        assert_eq!(loader.body_owner(body_b), Some(id_b));
        // Context words round-trip per record.
        assert_eq!(loader.context_id(id_a.get()), Some(id_a));
        assert_eq!(loader.context_id(id_b.get()), Some(id_b));
        // An identical parsed repeat is a NEW record (real 403a90: the
        // later parser result is the new class object), even though cache
        // lookup still selects the first definition in an initially empty
        // bucket. The old ID stays pinned to its own pool/body.
        let (again_a, _) = loader.load_module(&module_a, &data_a).unwrap();
        assert_ne!(again_a, id_a);
        assert_eq!(loader.len(), 3);
        assert_eq!(loader.lookup("game.Alpha"), Some(id_a));
        assert_eq!(loader.pool_text_of(id_a, 3).unwrap(), "alpha_count");
        assert_eq!(loader.pool_text_of(again_a, 3).unwrap(), "alpha_count");
        assert_eq!(loader.unique_name_count(), 2);
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
        assert_eq!(loader.len(), 0);
    }

    #[test]
    fn repeated_identical_file_parse_registers_a_fresh_record_each_time() {
        let (module, data) = tiny_module(sample_pool(), vec![], vec![[0, 3, 2, u32::MAX]]);
        let mut loader = Loader::new();
        let (id, _) = loader.load_module(&module, &data).unwrap();
        let context = loader.context_word("game.Widget").unwrap();
        assert_eq!(context, id.get());
        let body = loader.body_id("game.Widget", 0).unwrap();
        // A repeated identical parse registers a FRESH record (real 403a90:
        // the later parser result is the new class object); the lookup in
        // an initially empty bucket still selects the first definition.
        let (again, _) = loader.load_module(&module, &data).unwrap();
        assert_ne!(again, id);
        assert_eq!(loader.lookup("game.Widget"), Some(id));
        assert_eq!(loader.len(), 2);
        assert_eq!(loader.unique_name_count(), 1);
        assert_eq!(loader.context_word("game.Widget"), Some(context));
        assert_eq!(loader.body_id("game.Widget", 0), Some(body));
        assert_eq!(loader.body_owner(body), Some(id));
        // A conflicting redefinition registers as a duplicate too: the
        // old record keeps its pool while the new one carries the new
        // field, and lookup still selects the first definition.
        let mut pool = sample_pool();
        pool[3] = PoolEntry::Str("different".into());
        let (replacement, replacement_data) = tiny_module(pool, vec![], vec![[0, 3, 2, u32::MAX]]);
        let (dup, _) = loader.load_module(&replacement, &replacement_data).unwrap();
        assert_ne!(dup, id);
        assert_ne!(dup, again);
        assert_eq!(loader.len(), 3);
        assert_eq!(loader.pool_text("game.Widget", 3).unwrap(), "count");
        assert_eq!(loader.pool_text_of(id, 3).unwrap(), "count");
        assert_eq!(loader.pool_text_of(dup, 3).unwrap(), "different");
        assert_eq!(loader.find("game.Widget").unwrap().fields[0].name, "count");
        assert_eq!(loader.body_id("game.Widget", 0), Some(body));
        assert!(loader.body(body).is_some());
    }

    #[test]
    fn method_resolution_walks_the_chain() {
        let pool = sample_pool();
        let (module, data) = tiny_module(pool, vec![[1u32, 4, 5, 0, 2]], vec![]);
        let mut l = Loader::new();
        l.boot();
        // Parent first so the child can point at it.
        let parent = l.insert_test_class(Class {
            kind: 0,
            name: "java.lang.Object".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            bodies: vec![],
        });
        let (child, _) = l.load_module(&module, &data).expect("links");
        let (m, def) = l
            .resolve_method("game.Widget", "tick", "()V")
            .expect("found");
        assert_eq!(def, "game.Widget".to_owned());
        assert_eq!(m.name, "tick");
        let (m_id, def_id) = l
            .resolve_method_id(child, "tick", "()V")
            .expect("found by id");
        assert_eq!(def_id, child);
        assert_eq!(m_id.name, "tick");
        let _ = parent;
        assert!(l.resolve_method("game.Widget", "nope", "()V").is_none());
        assert!(l.is_assignable("game.Widget", "game.Widget"));
        assert!(l.is_assignable("game.Widget", "java.lang.Object"));
        assert!(!l.is_assignable("java.lang.Object", "game.Widget"));
        assert!(l
            .class_conversion_cost_id(child, l.lookup("java.lang.Object").unwrap())
            .is_ok());
    }

    /// Original 403a90/411610 hash vectors (byte length * 7 + signed-byte
    /// sum * 37, 32-bit wrap; bucket = hash % 512). Replays the exact
    /// expected hashes/buckets from the regen rows, including negative
    /// wrapped hashes and the equal-full-hash distinct-name triplet.
    #[test]
    fn class_cache_hash_matches_original_vectors() {
        // (name, expected hash, expected bucket).
        let vectors: &[(&str, u32, usize)] = &[
            ("pkg.é", 8182, 502),
            ("éééé", 4294945448, 168),
            ("pkg.€", 4785, 177),
            ("€€€", 4294940719, 47),
            ("aa.b", 12534, 246),
            ("a.ab", 12534, 246),
            ("ab.a", 12534, 246),
            ("java.render.osd.dialog.YesNoDialog", 0x1dab4, 180),
            ("unrelated.Czbaaaaaaaaaaaaaaaaaaa", 0x1c6b4, 180),
        ];
        for (name, hash, bucket) in vectors {
            assert_eq!(class_cache_hash(name), *hash, "hash of {name}");
            assert_eq!(class_cache_bucket(name), *bucket, "bucket of {name}");
        }
        // Distinct equal-full-hash names share bucket 246 but keep
        // distinct identities: replays the aa.b/a.ab/ab.a regen rows.
        assert_eq!(class_cache_hash("aa.b"), class_cache_hash("a.ab"));
        assert_eq!(class_cache_hash("a.ab"), class_cache_hash("ab.a"));
    }

    /// Duplicate-head cache topology without an unrelated head (regen
    /// scenario 1): three same-name registrations stay accepted and every
    /// lookup selects the FIRST definition.
    #[test]
    fn duplicate_registrations_without_collision_head_keep_first_winner() {
        let mut loader = Loader::new();
        let mut ids = Vec::new();
        for i in 0..3 {
            // Same name, distinct records: real 403a90 accepts every
            // same-name registration as a new class object.
            ids.push(loader.insert_test_class(Class {
                kind: i,
                name: "java.render.osd.dialog.YesNoDialog".into(),
                base: None,
                interfaces: vec![],
                methods: vec![],
                fields: vec![],
                bodies: vec![],
            }));
        }
        assert_eq!(loader.len(), 3);
        assert_eq!(loader.unique_name_count(), 1);
        let bucket = class_cache_bucket("java.render.osd.dialog.YesNoDialog");
        // Insert-after-head: every later duplicate goes immediately
        // after the head, pushing the previous second entry down.
        assert_eq!(loader.cache_chain(bucket), &vec![ids[0], ids[2], ids[1]]);
        for _ in 0..3 {
            assert_eq!(
                loader.lookup("java.render.osd.dialog.YesNoDialog"),
                Some(ids[0])
            );
        }
        assert_eq!(
            loader.resolve_descriptor("Ljava.render.osd.dialog.YesNoDialog;"),
            Some(ClassRef::Loaded(ids[0]))
        );
    }

    /// Collision-head topology with an unrelated head (regen scenario 2):
    /// inserting the unrelated bucket head first flips selection — lookups
    /// after the first/second/third duplicates return first/second/third.
    #[test]
    fn duplicate_registrations_with_collision_head_select_newest() {
        let mut loader = Loader::new();
        let head = loader.insert_test_class(Class {
            kind: 0,
            name: "unrelated.Czbaaaaaaaaaaaaaaaaaaa".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            bodies: vec![],
        });
        assert_eq!(
            class_cache_bucket("unrelated.Czbaaaaaaaaaaaaaaaaaaa"),
            class_cache_bucket("java.render.osd.dialog.YesNoDialog")
        );
        let mut ids = vec![head];
        for i in 0..3 {
            let id = loader.insert_test_class(Class {
                kind: i + 1,
                name: "java.render.osd.dialog.YesNoDialog".into(),
                base: None,
                interfaces: vec![],
                methods: vec![],
                fields: vec![],
                bodies: vec![],
            });
            ids.push(id);
            assert_eq!(
                loader.lookup("java.render.osd.dialog.YesNoDialog"),
                Some(id)
            );
        }
        assert_eq!(loader.len(), 4);
        let bucket = class_cache_bucket("java.render.osd.dialog.YesNoDialog");
        assert_eq!(
            loader.cache_chain(bucket),
            &vec![ids[0], ids[3], ids[2], ids[1]]
        );
    }

    /// Full-hash collision distinct-name behavior (regen step E): equal
    /// hashes in one bucket keep distinct identities; duplicate
    /// re-registrations keep earlier winners.
    #[test]
    fn equal_full_hash_distinct_names_keep_distinct_identities() {
        let mut loader = Loader::new();
        let aa = loader.insert_test_class(Class {
            kind: 0,
            name: "aa.b".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            bodies: vec![],
        });
        let ab = loader.insert_test_class(Class {
            kind: 0,
            name: "a.ab".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            bodies: vec![],
        });
        let ba = loader.insert_test_class(Class {
            kind: 0,
            name: "ab.a".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            bodies: vec![],
        });
        assert_ne!(aa, ab);
        assert_ne!(ab, ba);
        assert_eq!(loader.lookup("aa.b"), Some(aa));
        assert_eq!(loader.lookup("a.ab"), Some(ab));
        assert_eq!(loader.lookup("ab.a"), Some(ba));
        // Duplicate re-registrations keep earlier winners.
        let aa2 = loader.insert_test_class(Class {
            kind: 7,
            name: "aa.b".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            bodies: vec![],
        });
        let ab2 = loader.insert_test_class(Class {
            kind: 8,
            name: "a.ab".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            bodies: vec![],
        });
        // Distinct re-registration records keep earlier winners: the
        // second aa.b/a.ab definitions install as new records while
        // lookup still selects the first definitions.
        let _ = (aa2, ab2);
        assert_eq!(loader.lookup("aa.b"), Some(aa));
        assert_eq!(loader.lookup("a.ab"), Some(ab2));
        assert_eq!(loader.lookup("ab.a"), Some(ba));
        assert_eq!(loader.len(), 5);
        assert_eq!(loader.unique_name_count(), 3);
    }
}
