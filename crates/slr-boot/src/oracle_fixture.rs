//! Test-only availability policy for local oracle captures and stock files.
//! Absence is optional in fixture-free CI; partial captures still reach the
//! strict loaders and fail. Production replay loaders never use this policy.
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub(crate) fn clock_capture() -> Option<PathBuf> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../re/out/headless/clock-investigation/menu1800M");
    locate(&base, required())
}
fn required() -> bool {
    std::env::var_os("SLR_REQUIRE_ORACLE_FIXTURES").is_some()
}
#[allow(dead_code)]
pub(crate) fn stock_root() -> Option<PathBuf> {
    locate(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock"),
        required(),
    )
}
#[allow(dead_code)]
pub(crate) fn file(path: &Path) -> Option<PathBuf> {
    locate_kind(path, required(), false)
}
fn locate(base: &Path, required: bool) -> Option<PathBuf> {
    locate_kind(base, required, true)
}
fn locate_kind(base: &Path, required: bool, directory: bool) -> Option<PathBuf> {
    match std::fs::metadata(base) {
        Ok(metadata) => {
            assert!(
                if directory {
                    metadata.is_dir()
                } else {
                    metadata.is_file()
                },
                "oracle fixture has the wrong file type: {}",
                base.display()
            );
            Some(base.to_owned())
        }
        Err(error) if error.kind() == ErrorKind::NotFound && !required => {
            eprintln!("SKIP: local test fixture unavailable at {}; set SLR_REQUIRE_ORACLE_FIXTURES=1 to require it",base.display());
            None
        }
        Err(error) => panic!(
            "required/readable oracle capture {}: {error}",
            base.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "slr-oracle-availability-{}-{name}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        root
    }
    #[test]
    fn only_an_absent_capture_directory_is_optional() {
        let root = root("optional");
        assert!(locate(&root, false).is_none());
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(locate(&root, false), Some(root.clone()));
        std::fs::write(root.join("capture.json"), b"corrupt JSON").unwrap();
        let file = root.join("capture.json");
        assert_eq!(locate_kind(&file, false, false), Some(file.clone()));
        assert!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(file).unwrap()).is_err()
        );
        assert!(locate_kind(&root.join("missing.json"), false, false).is_none());
        assert!(std::panic::catch_unwind(|| locate_kind(&root, false, false)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn required_capture_cannot_be_silently_skipped() {
        let root = root("required");
        assert!(std::panic::catch_unwind(|| locate(&root, true)).is_err());
    }
    #[test]
    fn non_directory_capture_cannot_be_silently_skipped() {
        let root = root("file");
        std::fs::write(&root, b"not a directory").unwrap();
        assert!(std::panic::catch_unwind(|| locate(&root, false)).is_err());
        std::fs::remove_file(root).unwrap();
    }
}
