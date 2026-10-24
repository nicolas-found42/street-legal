use std::path::Path;

pub fn read(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && std::env::var_os("SLR_REQUIRE_ORACLE_FIXTURES").is_none() =>
        {
            eprintln!("SKIP: local oracle unavailable at {}; set SLR_REQUIRE_ORACLE_FIXTURES=1 to require it", path.display());
            None
        }
        Err(error) => panic!("required/readable oracle {}: {error}", path.display()),
    }
}
