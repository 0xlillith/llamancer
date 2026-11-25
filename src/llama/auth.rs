//! API credentials are read from a private file, never serialized or passed as literal argv.
use std::{
    fs::OpenOptions,
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use url::Url;

pub struct Key(String);
impl Key {
    pub fn read(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::other("API key file must be an absolute path"));
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let meta = file.metadata()?;
        // SAFETY: geteuid has no arguments or memory preconditions.
        let uid = unsafe { libc::geteuid() };
        if !meta.is_file()
            || meta.nlink() != 1
            || meta.uid() != uid
            || meta.mode() & 0o077 != 0
            || meta.len() > 4096
        {
            return Err(io::Error::other(
                "API key file must be a private regular file owned by the current user (0600)",
            ));
        }
        let mut text = String::new();
        file.take(4097).read_to_string(&mut text)?;
        let token = text.trim_end_matches(['\r', '\n']);
        if token.is_empty()
            || token.len() > 4096
            || token.starts_with('#')
            || !token.bytes().all(|b| b.is_ascii_graphic() && b != b',')
        {
            return Err(io::Error::other(
                "API key file must contain exactly one nonempty printable token",
            ));
        }
        Ok(Self(token.into()))
    }
    pub fn for_endpoint(path: Option<&Path>, url: &Url) -> io::Result<Option<Self>> {
        let Some(path) = path else {
            return Ok(None);
        };
        if url.scheme() != "https"
            && !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        {
            return Err(io::Error::other(
                "API credentials require HTTPS or a loopback endpoint",
            ));
        }
        Self::read(path).map(Some)
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
    pub fn temporary_file(&self) -> io::Result<tempfile::NamedTempFile> {
        let mut file = tempfile::NamedTempFile::new()?;
        writeln!(file, "{}", self.0)?;
        file.as_file().sync_all()?;
        Ok(file)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_key_only() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "test-token").unwrap();
        assert_eq!(Key::read(file.path()).unwrap().expose(), "test-token");
        assert!(Key::for_endpoint(
            Some(file.path()),
            &Url::parse("http://example.com").unwrap()
        )
        .is_err());
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(Key::read(file.path()).is_err());
    }
}
