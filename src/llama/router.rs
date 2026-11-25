//! Router presets use a temporary INI kept alive for the owned server.
use super::process;
use crate::{
    config::Config,
    models::{group::Group, library::identity},
};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::{self, Write},
    os::unix::ffi::OsStrExt,
    path::Path,
};
use tempfile::NamedTempFile;
pub fn id(group: &Group) -> String {
    let hash = Sha256::digest(identity(group).as_os_str().as_bytes());
    let slug: String = group
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .take(72)
        .collect();
    format!("{}-{}", slug, &format!("{hash:x}")[..16])
}
fn ini_value(value: &std::ffi::OsStr) -> io::Result<&str> {
    let value = value
        .to_str()
        .ok_or_else(|| io::Error::other("Router requires UTF-8 paths"))?;
    if value.trim() != value
        || value
            .chars()
            .any(|c| c.is_control() || matches!(c, ';' | '#'))
    {
        return Err(io::Error::other(
            "Path cannot be represented safely in a router preset",
        ));
    }
    Ok(value)
}
pub fn prepare(
    groups: &[Group],
    config: &Config,
    help: &str,
) -> io::Result<(NamedTempFile, Vec<OsString>)> {
    for flag in ["--models-preset", "--models-max", "--no-models-autoload"] {
        if !help.contains(flag) {
            return Err(io::Error::other(format!(
                "Router capability missing: {flag}"
            )));
        }
    }
    let mut file = NamedTempFile::new()?;
    writeln!(file, "version = 1")?;
    let mut count = 0;
    for group in groups.iter().filter(|g| g.complete && !g.projector) {
        let path = &group.files[0].path;
        let profile = config
            .profiles
            .get(&identity(group))
            .cloned()
            .unwrap_or_default();
        let args = process::args(path, &config.endpoint, &profile, help)?;
        writeln!(
            file,
            "\n[{}]\nmodel = {}",
            id(group),
            ini_value(path.as_os_str())?
        )?;
        let mut iter = args[8..].iter();
        while let Some(flag) = iter.next() {
            let flag = flag
                .to_str()
                .ok_or_else(|| io::Error::other("Invalid preset flag"))?;
            let key = flag
                .strip_prefix("--")
                .ok_or_else(|| io::Error::other("Invalid preset flag"))?;
            if key == "jinja" {
                writeln!(file, "jinja = true")?;
            } else {
                let value = iter
                    .next()
                    .ok_or_else(|| io::Error::other("Missing preset value"))?;
                writeln!(file, "{key} = {}", ini_value(value)?)?;
            }
        }
        count += 1;
    }
    if count == 0 {
        return Err(io::Error::other(
            "Scan at least one complete model before starting router",
        ));
    }
    file.as_file().sync_all()?;
    let base = process::args(
        Path::new("unused"),
        &config.endpoint,
        &Default::default(),
        help,
    )?;
    let mut args = base[2..6].to_vec();
    args.extend([
        "--models-preset".into(),
        file.path().as_os_str().to_owned(),
        "--models-max".into(),
        "1".into(),
        "--no-models-autoload".into(),
    ]);
    Ok((file, args))
}
