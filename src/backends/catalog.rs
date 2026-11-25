//! Reviewed upstream archive metadata and bounded receipt inspection.
use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
pub const RELEASE: &str = "b12721";
pub const RELEASES: [&str; 2] = [RELEASE, "b12726"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cpu,
    Vulkan,
    Rocm,
    Cuda,
}
impl Kind {
    pub const ALL: [Self; 4] = [Self::Cpu, Self::Vulkan, Self::Rocm, Self::Cuda];
    pub fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Vulkan => "vulkan",
            Self::Rocm => "rocm",
            Self::Cuda => "cuda",
        }
    }
    pub fn parse(name: &str) -> io::Result<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.name() == name)
            .ok_or_else(|| io::Error::other("Choose cpu, vulkan, rocm or cuda"))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    pub kind: Kind,
    pub release: &'static str,
    pub suffix: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}
#[derive(Debug, Clone)]
pub(super) struct Archive {
    pub filename: String,
    pub size: u64,
    pub sha256: &'static str,
}
impl Archive {
    pub fn url(&self, release: &str) -> String {
        format!(
            "https://github.com/ggml-org/llama.cpp/releases/download/{release}/{}",
            self.filename
        )
    }
}
fn receipt_matches(path: &Path, digest: &str) -> bool {
    let Ok(file) = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    else {
        return false;
    };
    if !file.metadata().is_ok_and(|m| m.is_file() && m.len() <= 65) {
        return false;
    }
    let mut receipt = String::new();
    file.take(66).read_to_string(&mut receipt).is_ok() && receipt.trim() == digest
}
impl Asset {
    pub(super) fn primary(self) -> Archive {
        Archive {
            filename: self.filename(),
            size: self.size,
            sha256: self.sha256,
        }
    }
    pub(super) fn companion(self) -> Option<Archive> {
        if self.kind != Kind::Cuda {
            return None;
        }
        let (size, sha256) = match (self.release, self.suffix) {
            (RELEASE, "cuda-12.8-x64") => (
                594373628,
                "1d77ececc77aaed45dbb23935391dd2cf3eff78cdb6bbe0f0458caa814d4f8f1",
            ),
            ("b12726", "cuda-12.8-x64") => (
                594373586,
                "87ad8956cfb60099ee0e471c5c55249f201ca2a906b7ea61b11516982b37d5e0",
            ),
            (RELEASE, "cuda-13.5-arm64") => (
                559872931,
                "2a70f890e0a5661796b9d25ca45cac0099d85904e87797d2d2c073f18afbc3be",
            ),
            ("b12726", "cuda-13.5-arm64") => (
                559873106,
                "ecc652b36825f80266377d650f03af0fa737fa82943277c5c8f0c223b61f674f",
            ),
            _ => return None,
        };
        Some(Archive {
            filename: format!("cudart-{}", self.filename()),
            size,
            sha256,
        })
    }
    pub fn total_size(self) -> u64 {
        self.size + self.companion().map_or(0, |a| a.size)
    }

    pub fn filename(self) -> String {
        format!("llama-{}-bin-ubuntu-{}.tar.gz", self.release, self.suffix)
    }
    pub fn url(self) -> String {
        format!(
            "https://github.com/ggml-org/llama.cpp/releases/download/{}/{}",
            self.release,
            self.filename()
        )
    }
    pub fn directory(self, root: &Path) -> PathBuf {
        root.join(format!("{}-{}", self.release, self.suffix))
    }
    pub fn binary(self, root: &Path) -> PathBuf {
        self.directory(root)
            .join(format!("llama-{}/llama-server", self.release))
    }
    pub fn installed(self, root: &Path) -> bool {
        if !self.valid() {
            return false;
        }
        let dir = self.directory(root);
        let binary = self.binary(root);
        if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir())
            || !binary
                .parent()
                .is_some_and(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()))
            || !fs::symlink_metadata(&binary)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        {
            return false;
        }
        receipt_matches(&dir.join("sha256"), self.sha256)
            && self
                .companion()
                .is_none_or(|a| receipt_matches(&dir.join("cudart-sha256"), a.sha256))
    }
    pub(super) fn valid(self) -> bool {
        catalog_version(self.kind, self.release) == Some(self)
    }
}
pub fn catalog(kind: Kind) -> Option<Asset> {
    catalog_version(kind, RELEASE)
}
pub fn catalog_version(kind: Kind, tag: &str) -> Option<Asset> {
    let release = *RELEASES.iter().find(|&&r| r == tag)?;
    let (suffix, size, sha256) = match (release, std::env::consts::ARCH, kind) {
        (RELEASE, "x86_64", Kind::Cpu) => (
            "x64",
            23388595,
            "ab56524ce899526e51c04e77f089a5792c59282b49736b239900970d3947be47",
        ),
        (RELEASE, "x86_64", Kind::Vulkan) => (
            "vulkan-x64",
            37095907,
            "ba2a338af1aa1d82d74d23f7e50e19ca5cb4da064d54bd86f6c01c80f77e7bda",
        ),
        (RELEASE, "x86_64", Kind::Rocm) => (
            "rocm-10.0-x64",
            277131895,
            "bfe159abce6a526ec6b5cc8ba44a9e91600d3c7632b78c3d48733efe224937bb",
        ),
        (RELEASE, "aarch64", Kind::Cpu) => (
            "arm64",
            18944130,
            "2c615cee75536574a30002ead85a3a259eac2ed3519d5e22546e61a5a9430c40",
        ),
        (RELEASE, "aarch64", Kind::Vulkan) => (
            "vulkan-arm64",
            30074713,
            "d5c0356952fa73344ed2baab6a7a2aceff4a4f23005341e053eb7d5d222b553e",
        ),
        ("b12726", "x86_64", Kind::Cpu) => (
            "x64",
            23396793,
            "cbfac3a6328b62f33239698f6b4daca58597a7be9f2e67b53ac1d28877908f71",
        ),
        ("b12726", "x86_64", Kind::Vulkan) => (
            "vulkan-x64",
            37107018,
            "71c65dc9e8b24f3042d5b1709e2abc0ae1c098ddaffa029d85bcec319b24bc8a",
        ),
        ("b12726", "x86_64", Kind::Rocm) => (
            "rocm-10.0-x64",
            277168959,
            "786f13e7802971684be7b9cb67c6cc8c2c4eea95d11faac20f2d78141db116d0",
        ),
        ("b12726", "aarch64", Kind::Cpu) => (
            "arm64",
            18946594,
            "db9f2796bc7365e8d9b8eff41497ab5d817e39e1117c48e5ea02cd6712cf9f54",
        ),
        ("b12726", "aarch64", Kind::Vulkan) => (
            "vulkan-arm64",
            30074722,
            "74d5fd4ca137330d07f089ee4b0b1c44ba20b80b18906fb15a32213d3fed9ef2",
        ),
        (RELEASE, "x86_64", Kind::Cuda) => (
            "cuda-12.8-x64",
            220565387,
            "f150cc31b798b334a11682812897e287c3e8041912df9936f95d52a2f9d5f7f5",
        ),
        ("b12726", "x86_64", Kind::Cuda) => (
            "cuda-12.8-x64",
            220565552,
            "baf0091b291df7ca5ee12fbc1de9dfa0468b9fef75f498e3e55467c587f1d356",
        ),
        (RELEASE, "aarch64", Kind::Cuda) => (
            "cuda-13.5-arm64",
            219835078,
            "06671e4280941efce5cffb3fa34cc8173bb2a9c2548ed0a6970205f8d501d252",
        ),
        ("b12726", "aarch64", Kind::Cuda) => (
            "cuda-13.5-arm64",
            219839714,
            "24623474213d21839f9149afb1a844a7d96d1e20b1f5eeb79b061c62f400b5d7",
        ),
        _ => return None,
    };
    Some(Asset {
        kind,
        release,
        suffix,
        size,
        sha256,
    })
}
