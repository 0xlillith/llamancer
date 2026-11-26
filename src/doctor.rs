//! Read-only, shareable local setup diagnostics; no subprocesses or network.
use crate::{backends, config::Config};
use std::{
    ffi::OsStr,
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub fn executable(name: &str, path: Option<&OsStr>) -> bool {
    let candidate = |p: &Path| {
        fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if name.is_empty() {
        return false;
    }
    if name.contains('/') {
        return candidate(Path::new(name));
    }
    path.is_some_and(|path| std::env::split_paths(path).any(|p| candidate(&p.join(name))))
}

fn report(config: &Config, backend_root: Option<&Path>, path: Option<&OsStr>) -> String {
    let mut lines = vec!["Llamancer local setup check (offline)".to_owned()];
    let mut check = |ok: bool, label: &str, hint: &str| {
        lines.push(if ok {
            format!("OK   {label}")
        } else {
            format!("WARN {label}: {hint}")
        });
    };
    check(
        executable(&config.server_binary, path),
        "Configured inference executable",
        "Select a backend in Settings > Backend Manager (B), or set an external executable.",
    );
    let roots = config.model_roots.iter().filter(|p| p.is_dir()).count();
    let imports = config.model_imports.iter().filter(|p| p.is_file()).count();
    check(
        roots + imports > 0,
        "Model roots/imports present",
        "Add a GGUF model root or file reference in Settings.",
    );
    check(
        roots == config.model_roots.len() && imports == config.model_imports.len(),
        "Configured model paths exist",
        "One or more model paths are missing or inaccessible.",
    );
    let agent = match config.default_agent.as_str() {
        "omp" => Some("omp"),
        "pi" | "pi-native" => Some("pi"),
        "custom" => Some(config.custom_agent.as_str()),
        _ => None,
    };
    check(
        agent.is_some_and(|a| executable(a, path)),
        "Configured optional agent executable",
        "Install the agent separately or change the agent in Settings.",
    );
    if let Some(project) = &config.project_dir {
        check(
            project.is_dir(),
            "Agent project directory",
            "Choose an existing project directory.",
        );
    }
    for (name, label) in [
        ("kitten", "Optional Kitty integration"),
        ("btop", "Optional resource monitor"),
        ("xdg-open", "Optional Web UI opener"),
    ] {
        check(
            executable(name, path),
            label,
            "Install separately if needed.",
        );
    }
    lines.push("Managed backend receipts (not runtime probes):".into());
    if let Some(root) = backend_root {
        for release in backends::RELEASES {
            for kind in backends::Kind::ALL {
                if let Some(asset) = backends::catalog_version(kind, release) {
                    lines.push(format!(
                        "  {release} {:7} {}",
                        kind.name(),
                        if asset.installed(root) {
                            "installed"
                        } else {
                            "not installed"
                        }
                    ));
                }
            }
        }
    } else {
        lines.push("  WARN XDG data directory unavailable".into());
    }
    lines.push("No paths, endpoint addresses or credentials included. Executable checks inspect metadata only; use backend probe for real device detection.".into());
    lines.join("\n")
}

fn load_config(root: &Path) -> io::Result<Config> {
    Config::load(&root.join("config.toml")).map_err(|_| {
        io::Error::other(
            "Cannot load Llamancer configuration; check config.toml format and permissions.",
        )
    })
}

pub fn run() -> io::Result<()> {
    let config = load_config(
        &crate::config::config_dir()
            .map_err(|_| io::Error::other("No XDG configuration directory"))?,
    )?;
    let root: Option<PathBuf> = backends::root().ok();
    let path = std::env::var_os("PATH");
    println!("{}", report(&config, root.as_deref(), path.as_deref()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn executable_resolution_and_redacted_report() {
        let tmp = tempfile::tempdir().unwrap();
        let binary = tmp.path().join("private-executable");
        fs::write(&binary, "not executed").unwrap();
        assert!(!executable(
            "private-executable",
            Some(tmp.path().as_os_str())
        ));
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(executable(
            "private-executable",
            Some(tmp.path().as_os_str())
        ));
        assert!(executable(binary.to_str().unwrap(), None));
        let config = Config {
            server_binary: binary.to_string_lossy().into_owned(),
            model_roots: vec![tmp.path().join("missing-private-root")],
            endpoint: "https://private.example/".into(),
            api_key_file: Some(tmp.path().join("private-token")),
            ..Config::default()
        };
        let result = report(&config, None, Some(tmp.path().as_os_str()));
        assert!(result.contains("OK   Configured inference executable"));
        assert!(result.contains("WARN Model roots/imports present"));
        assert!(!result.contains("private-"));
        assert!(!result.contains("private.example"));
    }
    #[test]
    fn missing_config_does_not_create_state_and_invalid_config_is_redacted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("missing");
        assert_eq!(load_config(&root).unwrap(), Config::default());
        assert!(!root.exists());
        fs::create_dir(&root).unwrap();
        fs::write(root.join("config.toml"), "secret-private-value !").unwrap();
        let error = load_config(&root).unwrap_err().to_string();
        assert!(!error.contains("secret-private-value"));
    }
}
