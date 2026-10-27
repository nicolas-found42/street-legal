//! Read-only platform leaves used by the source JVM namespace and class cache.
//!
//! Paths and serialized bytes come from the selected install. Handles belong
//! to this collaborator; semantic object IDs and oracle records are not inputs.
//! The declared headless filesystem profile admits ASCII names, deterministic
//! case-insensitive enumeration, and one explicitly owned semaphore. It does
//! not claim Win32 timestamps, short names, or arbitrary filesystem parity.
use slr_physics::space::Space;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug)]
pub enum FileLeaf {
    Create,
    Close,
    Size,
    Read,
    Wait,
    Release,
}

pub type FindEntry = (String, bool, u32);
struct Search {
    entries: Vec<FindEntry>,
    cursor: usize,
}
struct OpenFile {
    bytes: Vec<u8>,
    cursor: usize,
}

pub struct Files {
    root: PathBuf,
    searches: HashMap<u32, Search>,
    files: HashMap<u32, OpenFile>,
    next_search: u32,
    next_file: u32,
    semaphore: u32,
    acquired: bool,
}
impl Files {
    pub fn new(root: impl AsRef<Path>, semaphore: u32) -> Result<Self, String> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|e| format!("NativeVmPlatform: install root: {e}"))?;
        if !root.is_dir() || semaphore == 0 || semaphore == u32::MAX {
            return Err("NativeVmPlatform: install directory and owned semaphore required".into());
        }
        Ok(Self {
            root,
            searches: HashMap::new(),
            files: HashMap::new(),
            next_search: 0,
            next_file: 0x4000,
            semaphore,
            acquired: false,
        })
    }
    pub fn searches_closed(&self) -> bool {
        self.searches.is_empty()
    }
    fn string(m: &Space, address: u32) -> Result<String, String> {
        if address == 0 {
            return Err("NativeVmPlatform: null path".into());
        }
        let mut bytes = Vec::new();
        for i in 0..256 {
            let byte = m.r8(address
                .checked_add(i)
                .ok_or("NativeVmPlatform: path address overflow")?);
            if byte == 0 {
                if !bytes.is_ascii() {
                    return Err("NativeVmPlatform: non-ASCII path outside profile".into());
                }
                return String::from_utf8(bytes)
                    .map_err(|e| format!("NativeVmPlatform: path: {e}"));
            }
            bytes.push(byte);
        }
        Err("NativeVmPlatform: path exceeds source buffer".into())
    }
    fn resolve(&self, name: &str) -> Result<Option<PathBuf>, String> {
        let normalized = name.replace('\\', "/");
        if !normalized.is_ascii() || normalized.starts_with('/') || normalized.contains(':') {
            return Err(
                "NativeVmPlatform: absolute or non-ASCII path outside install profile".into(),
            );
        }
        let parts = normalized
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if parts
            .iter()
            .any(|s| matches!(*s, "." | "..") || s.contains(['*', '?']))
        {
            return Err("NativeVmPlatform: invalid relative path component".into());
        }
        let mut current = self.root.clone();
        for part in parts {
            let mut selected = None;
            for entry in std::fs::read_dir(&current)
                .map_err(|e| format!("NativeVmPlatform: directory: {e}"))?
            {
                let entry = entry.map_err(|e| format!("NativeVmPlatform: directory entry: {e}"))?;
                let name = entry.file_name();
                let text = name
                    .to_str()
                    .filter(|s| s.is_ascii())
                    .ok_or("NativeVmPlatform: non-ASCII install entry outside profile")?;
                if text.eq_ignore_ascii_case(part) && selected.replace(entry.path()).is_some() {
                    return Err("NativeVmPlatform: ambiguous case-insensitive install path".into());
                }
            }
            let Some(path) = selected else {
                return Ok(None);
            };
            current = path
                .canonicalize()
                .map_err(|e| format!("NativeVmPlatform: selected path: {e}"))?;
            if !current.starts_with(&self.root) {
                return Err("NativeVmPlatform: selected path escapes install".into());
            }
        }
        Ok(Some(current))
    }
    /// The deterministic FindFirstFile profile used by the source comparisons.
    /// The source54cf50 still performs its own original qsort and slot stores.
    pub fn entries(&self, pattern: &str) -> Result<Vec<FindEntry>, String> {
        let pattern = pattern.replace('\\', "/");
        let (directory, mask) = pattern.rsplit_once('/').unwrap_or(("", &pattern));
        if !matches!(mask, "*" | "*.*" | "*.java" | "*.class") {
            return Err("NativeVmPlatform: wildcard outside declared class-loader profile".into());
        }
        let Some(path) = self.resolve(directory)? else {
            return Ok(Vec::new());
        };
        let mut entries = vec![(".".into(), true, 0), ("..".into(), true, 0)];
        for entry in
            std::fs::read_dir(path).map_err(|e| format!("NativeVmPlatform: enumerate: {e}"))?
        {
            let entry = entry.map_err(|e| format!("NativeVmPlatform: enumerate entry: {e}"))?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .filter(|s| s.is_ascii())
                .ok_or("NativeVmPlatform: non-ASCII filename outside profile")?;
            if name.len() >= 260 {
                return Err("NativeVmPlatform: filename exceeds Win32 buffer".into());
            }
            let canonical = entry
                .path()
                .canonicalize()
                .map_err(|e| format!("NativeVmPlatform: entry path: {e}"))?;
            if !canonical.starts_with(&self.root) {
                return Err("NativeVmPlatform: enumerated path escapes install".into());
            }
            let metadata = entry
                .metadata()
                .map_err(|e| format!("NativeVmPlatform: entry metadata: {e}"))?;
            if !metadata.is_dir() && !metadata.is_file() {
                return Err("NativeVmPlatform: special file outside install profile".into());
            }
            let size = if metadata.is_dir() {
                0
            } else {
                u32::try_from(metadata.len())
                    .map_err(|_| "NativeVmPlatform: file exceeds32-bit size")?
            };
            entries.push((name.to_owned(), metadata.is_dir(), size));
        }
        entries.sort_by_key(|e| e.0.to_ascii_lowercase());
        if entries
            .windows(2)
            .any(|e| e[0].0.eq_ignore_ascii_case(&e[1].0))
        {
            return Err("NativeVmPlatform: ambiguous case-insensitive enumeration".into());
        }
        if let Some(extension) = mask.strip_prefix("*.").filter(|s| *s != "*") {
            entries.retain(|e| e.0.to_ascii_lowercase().ends_with(&format!(".{extension}")));
        }
        Ok(entries)
    }
    fn write_find(m: &mut Space, destination: u32, entry: &FindEntry) -> Result<(), String> {
        destination
            .checked_add(319)
            .filter(|_| destination != 0)
            .ok_or("NativeVmPlatform: find buffer address invalid")?;
        let mut bytes = [0u8; 320];
        bytes[..4].copy_from_slice(&(if entry.1 { 16u32 } else { 32u32 }).to_le_bytes());
        bytes[32..36].copy_from_slice(&entry.2.to_le_bytes());
        bytes[44..44 + entry.0.len()].copy_from_slice(entry.0.as_bytes());
        for (i, byte) in bytes.into_iter().enumerate() {
            m.w8(destination + i as u32, byte);
        }
        Ok(())
    }
    pub fn find_first(
        &mut self,
        m: &mut Space,
        pattern: u32,
        destination: u32,
    ) -> Result<u32, String> {
        let entries = self.entries(&Self::string(m, pattern)?)?;
        if entries.is_empty() {
            return Ok(u32::MAX);
        }
        let handle = self
            .next_search
            .checked_add(1)
            .filter(|v| *v < 0x3000)
            .ok_or("NativeVmPlatform: find handles exhausted")?;
        Self::write_find(m, destination, &entries[0])?;
        self.searches.insert(handle, Search { entries, cursor: 1 });
        self.next_search = handle;
        Ok(handle)
    }
    pub fn find_next(
        &mut self,
        m: &mut Space,
        handle: u32,
        destination: u32,
    ) -> Result<u32, String> {
        let search = self
            .searches
            .get_mut(&handle)
            .ok_or("NativeVmPlatform: foreign or closed find handle")?;
        let Some(entry) = search.entries.get(search.cursor) else {
            return Ok(0);
        };
        Self::write_find(m, destination, entry)?;
        search.cursor += 1;
        Ok(1)
    }
    pub fn find_close(&mut self, handle: u32) -> Result<u32, String> {
        self.searches
            .remove(&handle)
            .ok_or("NativeVmPlatform: foreign or closed find handle")?;
        Ok(1)
    }
    pub fn file_leaf(
        &mut self,
        m: &mut Space,
        leaf: FileLeaf,
        args: &[u32],
    ) -> Result<u32, String> {
        let expected = match leaf {
            FileLeaf::Create => 7,
            FileLeaf::Close => 1,
            FileLeaf::Size | FileLeaf::Wait => 2,
            FileLeaf::Read => 5,
            FileLeaf::Release => 3,
        };
        if args.len() != expected {
            return Err("NativeVmPlatform: file leaf argument count differs".into());
        }
        match leaf {
            FileLeaf::Create => {
                if args[1..] != [0x80000000, 1, 0, 3, 0x80, 0] {
                    return Err(
                        "NativeVmPlatform: unsupported file access outside read-only profile"
                            .into(),
                    );
                }
                let Some(path) = self.resolve(&Self::string(m, args[0])?)? else {
                    return Ok(u32::MAX);
                };
                if !path.is_file() {
                    return Ok(u32::MAX);
                }
                let bytes = std::fs::read(path)
                    .map_err(|e| format!("NativeVmPlatform: read original file: {e}"))?;
                u32::try_from(bytes.len())
                    .map_err(|_| "NativeVmPlatform: file exceeds32-bit size")?;
                let handle = self
                    .next_file
                    .checked_add(1)
                    .filter(|v| *v != u32::MAX && *v != self.semaphore)
                    .ok_or("NativeVmPlatform: file handles exhausted")?;
                self.files.insert(handle, OpenFile { bytes, cursor: 0 });
                self.next_file = handle;
                Ok(handle)
            }
            FileLeaf::Close => {
                self.files
                    .remove(&args[0])
                    .ok_or("NativeVmPlatform: foreign or closed file handle")?;
                Ok(1)
            }
            FileLeaf::Size => {
                let file = self
                    .files
                    .get(&args[0])
                    .ok_or("NativeVmPlatform: foreign or closed file handle")?;
                if args[1] != 0 {
                    args[1]
                        .checked_add(3)
                        .ok_or("NativeVmPlatform: size buffer address overflow")?;
                    m.w32(args[1], 0);
                }
                Ok(file.bytes.len() as u32)
            }
            FileLeaf::Read => {
                if args[4] != 0 {
                    return Err("NativeVmPlatform: asynchronous read outside profile".into());
                }
                let file = self
                    .files
                    .get_mut(&args[0])
                    .ok_or("NativeVmPlatform: foreign or closed file handle")?;
                let count = (args[2] as usize).min(file.bytes.len() - file.cursor);
                if args[1] == 0
                    || args[3] == 0
                    || args[1].checked_add(count as u32).is_none()
                    || args[3].checked_add(3).is_none()
                {
                    return Err("NativeVmPlatform: read buffer address invalid".into());
                }
                for (i, byte) in file.bytes[file.cursor..file.cursor + count]
                    .iter()
                    .enumerate()
                {
                    m.w8(args[1] + i as u32, *byte);
                }
                m.w32(args[3], count as u32);
                file.cursor += count;
                Ok(1)
            }
            FileLeaf::Wait => {
                if args != [self.semaphore, u32::MAX] || self.acquired {
                    return Err(
                        "NativeVmPlatform: unavailable or foreign semaphore acquisition".into(),
                    );
                }
                self.acquired = true;
                Ok(0)
            }
            FileLeaf::Release => {
                if args[0] != self.semaphore || args[1] != 1 || !self.acquired {
                    return Err("NativeVmPlatform: unowned semaphore release".into());
                }
                if args[2] != 0 {
                    args[2]
                        .checked_add(3)
                        .ok_or("NativeVmPlatform: semaphore output overflow")?;
                    m.w32(args[2], 0);
                }
                self.acquired = false;
                Ok(1)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static NEXT_INSTALL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Install(PathBuf);
    impl Install {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "slr-native-platform-{}-{stamp}-{}",
                std::process::id(),
                NEXT_INSTALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Install {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn string(m: &mut Space, address: u32, bytes: &[u8]) {
        for (i, byte) in bytes.iter().chain(std::iter::once(&0)).enumerate() {
            m.w8(address + i as u32, *byte);
        }
    }
    fn open(files: &mut Files, m: &mut Space, path: &[u8]) -> Result<u32, String> {
        string(m, 0x1000, path);
        files.file_leaf(m, FileLeaf::Create, &[0x1000, 0x80000000, 1, 0, 3, 0x80, 0])
    }
    #[test]
    fn original_bytes_partial_reads_and_handle_retirement_preserve_owner_and_cursor() {
        let install = Install::new();
        std::fs::create_dir(install.0.join("Scripts")).unwrap();
        let bytes = vec![0, 255, 13, 10, 128, 1];
        std::fs::write(install.0.join("Scripts/Input.class"), &bytes).unwrap();
        let mut files = Files::new(&install.0, 0x3000).unwrap();
        let mut m = Space::new();
        let handle = open(&mut files, &mut m, b"scripts\\INPUT.CLASS").unwrap();
        assert_eq!(handle, 0x4001);
        assert!(files
            .file_leaf(&mut m, FileLeaf::Read, &[handle, 0, 3, 0x3000, 0])
            .is_err());
        assert_eq!(
            files
                .file_leaf(&mut m, FileLeaf::Read, &[handle, 0x2000, 3, 0x3000, 0])
                .unwrap(),
            1
        );
        assert_eq!(m.r32(0x3000), 3);
        assert_eq!(
            (0..3).map(|i| m.r8(0x2000 + i)).collect::<Vec<_>>(),
            bytes[..3]
        );
        assert_eq!(
            files
                .file_leaf(&mut m, FileLeaf::Read, &[handle, 0x2010, 100, 0x3000, 0])
                .unwrap(),
            1
        );
        assert_eq!(m.r32(0x3000), 3);
        assert_eq!(
            (0..3).map(|i| m.r8(0x2010 + i)).collect::<Vec<_>>(),
            bytes[3..]
        );
        assert_eq!(
            files
                .file_leaf(&mut m, FileLeaf::Read, &[handle, 0x2010, 100, 0x3000, 0])
                .unwrap(),
            1
        );
        assert_eq!(m.r32(0x3000), 0);
        assert_eq!(
            files
                .file_leaf(&mut m, FileLeaf::Size, &[handle, 0x3010])
                .unwrap(),
            6
        );
        assert_eq!(m.r32(0x3010), 0);
        files.file_leaf(&mut m, FileLeaf::Close, &[handle]).unwrap();
        assert!(files
            .file_leaf(&mut m, FileLeaf::Read, &[handle, 0x2000, 1, 0x3000, 0])
            .is_err());
        assert!(files.file_leaf(&mut m, FileLeaf::Close, &[handle]).is_err());
        assert_ne!(
            open(&mut files, &mut m, b"Scripts/Input.class").unwrap(),
            handle
        );
        assert_eq!(
            open(&mut files, &mut m, b"Scripts/missing.class").unwrap(),
            u32::MAX
        );
        assert_eq!(
            std::fs::read(install.0.join("Scripts/Input.class")).unwrap(),
            bytes
        );
    }
    #[test]
    fn enumeration_has_explicit_profile_and_rejects_ambiguity_without_consuming_a_handle() {
        let install = Install::new();
        std::fs::write(install.0.join("z.class"), [1, 2]).unwrap();
        std::fs::write(install.0.join("A.class"), [3]).unwrap();
        std::fs::write(install.0.join("b.java"), []).unwrap();
        let mut files = Files::new(&install.0, 0x3000).unwrap();
        assert_eq!(
            files.entries("*.class").unwrap(),
            vec![("A.class".into(), false, 1), ("z.class".into(), false, 2)]
        );
        let mut m = Space::new();
        string(&mut m, 0x1000, b"*.class");
        assert!(files.find_first(&mut m, 0x1000, 0).is_err());
        let handle = files.find_first(&mut m, 0x1000, 0x2000).unwrap();
        assert_eq!(handle, 1);
        assert_eq!(m.r32(0x2000), 32);
        assert_eq!(m.r32(0x2020), 1);
        assert_eq!(m.r8(0x202c), b'A');
        assert!(files.find_next(&mut m, handle, 0).is_err());
        assert_eq!(files.find_next(&mut m, handle, 0x2000).unwrap(), 1);
        assert_eq!(m.r8(0x202c), b'z');
        assert_eq!(files.find_next(&mut m, handle, 0x2000).unwrap(), 0);
        files.find_close(handle).unwrap();
        assert!(files.find_next(&mut m, handle, 0x2000).is_err());
        assert!(files.searches_closed());
        assert!(files.entries("../*.class").is_err());
        assert!(files.entries("?.class").is_err());
        // The host filesystem may be case-insensitive. Only run this collision
        // assertion if it actually stored two separate names.
        std::fs::write(install.0.join("a.CLASS"), [4]).unwrap();
        if std::fs::read_dir(&install.0).unwrap().count() == 4 {
            assert!(files.entries("*.class").is_err());
            assert!(open(&mut files, &mut m, b"a.class").is_err());
        }
    }
    #[test]
    fn semaphore_and_path_rejections_do_not_mutate_memory_or_consume_acquisition() {
        let install = Install::new();
        let mut files = Files::new(&install.0, 0x3000).unwrap();
        let mut m = Space::new();
        m.w32(0x2000, 0xdeadbeef);
        assert!(files
            .file_leaf(&mut m, FileLeaf::Wait, &[0x3001, u32::MAX])
            .is_err());
        files
            .file_leaf(&mut m, FileLeaf::Wait, &[0x3000, u32::MAX])
            .unwrap();
        assert!(files
            .file_leaf(&mut m, FileLeaf::Wait, &[0x3000, u32::MAX])
            .is_err());
        assert!(files
            .file_leaf(&mut m, FileLeaf::Release, &[0x3001, 1, 0x2000])
            .is_err());
        assert_eq!(m.r32(0x2000), 0xdeadbeef);
        assert!(files
            .file_leaf(&mut m, FileLeaf::Release, &[0x3000, 1, u32::MAX])
            .is_err());
        files
            .file_leaf(&mut m, FileLeaf::Release, &[0x3000, 1, 0x2000])
            .unwrap();
        assert_eq!(m.r32(0x2000), 0);
        assert!(files
            .file_leaf(&mut m, FileLeaf::Release, &[0x3000, 1, 0x2000])
            .is_err());
        assert!(open(&mut files, &mut m, b"../outside").is_err());
        assert!(open(&mut files, &mut m, b"C:\\outside").is_err());
        assert!(open(&mut files, &mut m, b"/outside").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_outside_install_is_rejected_for_read_and_enumeration() {
        let install = Install::new();
        let other = Install::new();
        std::fs::write(other.0.join("secret.class"), [1, 2, 3]).unwrap();
        std::os::unix::fs::symlink(other.0.join("secret.class"), install.0.join("escape.class"))
            .unwrap();
        let mut files = Files::new(&install.0, 0x3000).unwrap();
        assert!(files.entries("*.class").is_err());
        assert!(open(&mut files, &mut Space::new(), b"escape.class").is_err());
    }
}
