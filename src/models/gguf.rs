//! Bounded little-endian GGUF v2/v3 metadata and tensor descriptor reader.
//! Format: https://github.com/ggml-org/ggml/blob/master/docs/gguf.md
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub version: u32,
    pub tensors: u64,
    pub elements: u64,
    pub name: Option<String>,
    pub architecture: Option<String>,
    pub file_type: Option<u64>,
    pub context: Option<u64>,
    pub layers: Option<u64>,
    pub tokenizer: Option<String>,
    pub vocabulary: Option<u64>,
    pub split_no: Option<u64>,
    pub split_count: Option<u64>,
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
struct Reader<R> {
    inner: R,
    pos: u64,
    end: u64,
}
impl<R: Read + Seek> Reader<R> {
    fn read<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        self.bound(N as u64)?;
        let mut bytes = [0; N];
        self.inner.read_exact(&mut bytes)?;
        self.pos += N as u64;
        Ok(bytes)
    }
    fn bound(&self, n: u64) -> io::Result<()> {
        if n > self.end.saturating_sub(self.pos) {
            Err(invalid("Truncated GGUF or header budget exceeded"))
        } else {
            Ok(())
        }
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.read()?))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.read()?))
    }
    fn skip(&mut self, n: u64) -> io::Result<()> {
        self.bound(n)?;
        self.inner.seek(SeekFrom::Start(self.pos + n))?;
        self.pos += n;
        Ok(())
    }
    fn string(&mut self, keep: bool) -> io::Result<Option<String>> {
        let n = self.u64()?;
        if !keep {
            self.skip(n)?;
            return Ok(None);
        }
        if n > 65536 {
            return Err(invalid("GGUF string too long"));
        }
        self.bound(n)?;
        let mut v = vec![0; n as usize];
        self.inner.read_exact(&mut v)?;
        self.pos += n;
        String::from_utf8(v)
            .map(Some)
            .map_err(|_| invalid("Invalid GGUF UTF-8"))
    }
    fn number(&mut self, t: u32) -> io::Result<Option<u64>> {
        match t {
            0 => Ok(Some(self.read::<1>()?[0] as u64)),
            2 => Ok(Some(u16::from_le_bytes(self.read()?) as u64)),
            4 => Ok(Some(self.u32()? as u64)),
            10 => Ok(Some(self.u64()?)),
            1 | 7 => {
                self.skip(1)?;
                Ok(None)
            }
            3 => {
                self.skip(2)?;
                Ok(None)
            }
            5 | 6 => {
                self.skip(4)?;
                Ok(None)
            }
            11 | 12 => {
                self.skip(8)?;
                Ok(None)
            }
            _ => Err(invalid("Unknown GGUF metadata type")),
        }
    }
}
fn cancelled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"))
    } else {
        Ok(())
    }
}
pub fn read(path: &Path, cancel: &AtomicBool) -> io::Result<Metadata> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut r = Reader {
        inner: BufReader::new(file),
        pos: 0,
        end: len.min(512 * 1024 * 1024),
    };
    if &r.read::<4>()? != b"GGUF" {
        return Err(invalid("Invalid GGUF magic"));
    }
    let version = r.u32()?;
    if !matches!(version, 2 | 3) {
        return Err(invalid("Unsupported GGUF version or byte order"));
    }
    let tensors = r.u64()?;
    let count = r.u64()?;
    if tensors > 1_000_000 || count > 100_000 {
        return Err(invalid("GGUF entry limit exceeded"));
    }
    let mut m = Metadata {
        version,
        tensors,
        ..Default::default()
    };
    let mut numbers = BTreeMap::new();
    let mut alignment = 32u64;
    for _ in 0..count {
        cancelled(cancel)?;
        let key = r.string(true)?.ok_or_else(|| invalid("Missing key"))?;
        if key.len() > 1024 {
            return Err(invalid("GGUF key too long"));
        }
        let t = r.u32()?;
        match t {
            8 => {
                let keep = matches!(
                    key.as_str(),
                    "general.name" | "general.architecture" | "tokenizer.ggml.model"
                );
                let value = r.string(keep)?;
                match key.as_str() {
                    "general.name" => m.name = value,
                    "general.architecture" => m.architecture = value,
                    "tokenizer.ggml.model" => m.tokenizer = value,
                    _ => {}
                }
            }
            9 => {
                let kind = r.u32()?;
                let n = r.u64()?;
                if n > 10_000_000 {
                    return Err(invalid("GGUF array limit exceeded"));
                }
                if key == "tokenizer.ggml.tokens" {
                    m.vocabulary = Some(n);
                }
                if kind == 8 {
                    for _ in 0..n {
                        cancelled(cancel)?;
                        r.string(false)?;
                    }
                } else {
                    let width = match kind {
                        0 | 1 | 7 => 1,
                        2 | 3 => 2,
                        4..=6 => 4,
                        10..=12 => 8,
                        _ => return Err(invalid("Unsupported GGUF array type")),
                    };
                    r.skip(
                        n.checked_mul(width)
                            .ok_or_else(|| invalid("Array size overflow"))?,
                    )?;
                }
            }
            _ => {
                if let Some(n) = r.number(t)? {
                    match key.as_str() {
                        "general.file_type" => m.file_type = Some(n),
                        "general.alignment" => alignment = n,
                        "split.no" => m.split_no = Some(n),
                        "split.count" => m.split_count = Some(n),
                        _ => {}
                    }
                    if key.len() <= 256
                        && (key.ends_with(".context_length") || key.ends_with(".block_count"))
                    {
                        if numbers.len() >= 4096 {
                            return Err(invalid("Too many architecture fields"));
                        }
                        numbers.insert(key, n);
                    }
                }
            }
        }
    }
    if let Some(a) = &m.architecture {
        m.context = numbers.get(&format!("{a}.context_length")).copied();
        m.layers = numbers.get(&format!("{a}.block_count")).copied();
    }
    if alignment == 0 || !alignment.is_power_of_two() || alignment > 1024 * 1024 {
        return Err(invalid("Invalid tensor alignment"));
    }
    let mut max_offset = None::<u64>;
    for _ in 0..tensors {
        cancelled(cancel)?;
        r.string(false)?;
        let dims = r.u32()?;
        if !(1..=4).contains(&dims) {
            return Err(invalid("Invalid tensor rank"));
        }
        let mut elements = 1u64;
        for _ in 0..dims {
            let n = r.u64()?;
            if n == 0 {
                return Err(invalid("Empty tensor dimension"));
            }
            elements = elements
                .checked_mul(n)
                .ok_or_else(|| invalid("Tensor size overflow"))?;
        }
        m.elements = m
            .elements
            .checked_add(elements)
            .ok_or_else(|| invalid("Element count overflow"))?;
        let _kind = r.u32()?;
        let offset = r.u64()?;
        if offset % alignment != 0 {
            return Err(invalid("Unaligned tensor offset"));
        }
        max_offset = Some(max_offset.map_or(offset, |old| old.max(offset)));
    }
    let start = r
        .pos
        .checked_add(alignment - 1)
        .ok_or_else(|| invalid("Alignment overflow"))?
        & !(alignment - 1);
    if let Some(offset) = max_offset {
        if start.checked_add(offset).is_none_or(|p| p >= len) {
            return Err(invalid("Tensor offset outside file"));
        }
    }
    Ok(m)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn string(b: &mut Vec<u8>, s: &str) {
        b.extend((s.len() as u64).to_le_bytes());
        b.extend(s.as_bytes());
    }
    #[test]
    fn metadata_and_truncation() {
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(3u64.to_le_bytes());
        string(&mut b, "general.architecture");
        b.extend(8u32.to_le_bytes());
        string(&mut b, "llama");
        string(&mut b, "llama.context_length");
        b.extend(4u32.to_le_bytes());
        b.extend(32768u32.to_le_bytes());
        string(&mut b, "tokenizer.ggml.tokens");
        b.extend(9u32.to_le_bytes());
        b.extend(8u32.to_le_bytes());
        b.extend(2u64.to_le_bytes());
        string(&mut b, "a");
        string(&mut b, "b");
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&b).unwrap();
        let cancel = AtomicBool::new(false);
        let m = read(f.path(), &cancel).unwrap();
        assert_eq!(m.context, Some(32768));
        assert_eq!(m.vocabulary, Some(2));
        f.as_file().set_len(b.len() as u64 - 1).unwrap();
        assert!(read(f.path(), &cancel).is_err());
    }
    #[test]
    fn huge_count_rejected() {
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(u64::MAX.to_le_bytes());
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&b).unwrap();
        assert!(read(f.path(), &AtomicBool::new(false)).is_err());
    }
}
