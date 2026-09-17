use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Enforce the byte limit on the opened stream, including files that grow
/// between discovery and reading. Never allocate for the untrusted file size.
pub(crate) fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds the size limit",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_files_accept_the_limit_and_reject_extra_bytes_and_directories() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("definition.json");
        std::fs::write(&path, b"{}").unwrap();
        assert_eq!(read_bounded(&path, 2).unwrap(), b"{}");
        assert_eq!(
            read_bounded(&path, 1).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(read_bounded(directory.path(), 10).is_err());
        std::fs::write(&path, b"").unwrap();
        assert!(read_bounded(&path, 0).unwrap().is_empty());
    }
}
