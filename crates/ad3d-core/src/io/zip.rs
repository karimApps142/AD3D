//! Minimal ZIP container for `.ad3d` projects and 3MF packages.
//!
//! Writer: entries are streamed (stored or deflated) straight to the output; the
//! local header's CRC and sizes are patched with one seek after each entry, so
//! no entry is ever held whole in memory and no data descriptors are needed
//! (which every 3MF consumer accepts). Timestamps are fixed (1980-01-01) so the
//! same content gives the same bytes. No ZIP64: an archive or entry past 4 GiB
//! is refused with an error.
//!
//! Reader: parses the central directory, then streams an entry through a CRC
//! check (stored or deflate only; encrypted entries are refused).

use std::io::{self, Read, Seek, SeekFrom, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use crate::error::{Error, Result};

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;
/// General-purpose flag bit 11: names are UTF-8.
const FLAG_UTF8: u16 = 1 << 11;
/// MS-DOS date for 1980-01-01 (day 1, month 1, year 0), time 00:00:00.
const DOS_DATE: u16 = (1 << 5) | 1;
const DOS_TIME: u16 = 0;

/// Compression method of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Stored,
    Deflate,
}

impl Method {
    fn code(self) -> u16 {
        match self {
            Method::Stored => 0,
            Method::Deflate => 8,
        }
    }
}

struct Counting<W> {
    w: W,
    n: u64,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let k = self.w.write(buf)?;
        self.n += k as u64;
        Ok(k)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

enum Sink<W: Write> {
    Plain(Counting<W>),
    Deflate(DeflateEncoder<Counting<W>>),
    Poisoned,
}

struct Open {
    name: String,
    method: Method,
    header_off: u64,
    data_start: u64,
    crc: crc32fast::Hasher,
    size: u64,
}

struct CentralEntry {
    name: String,
    method: Method,
    crc: u32,
    csize: u32,
    usize: u32,
    offset: u32,
}

/// Streaming ZIP writer. Use [`begin`](Self::begin), write the entry's bytes
/// through [`Write`], then [`end`](Self::end); or [`add`](Self::add) for small
/// entries. [`finish`](Self::finish) writes the central directory.
pub struct ZipWriter<W: Write + Seek> {
    sink: Sink<W>,
    open: Option<Open>,
    entries: Vec<CentralEntry>,
}

fn too_big() -> Error {
    Error::Io("zip: archive or entry exceeds 4 GiB (ZIP64 is not supported)".into())
}

fn u32_of(x: u64) -> Result<u32> {
    u32::try_from(x).map_err(|_| too_big())
}

impl<W: Write + Seek> ZipWriter<W> {
    /// Starts an archive at the writer's current position (normally 0).
    pub fn new(mut w: W) -> Result<Self> {
        let n = w.stream_position()?;
        Ok(ZipWriter { sink: Sink::Plain(Counting { w, n }), open: None, entries: Vec::new() })
    }

    fn plain(&mut self) -> Result<&mut Counting<W>> {
        match &mut self.sink {
            Sink::Plain(c) => Ok(c),
            _ => Err(Error::Internal("zip: entry still open".into())),
        }
    }

    /// Opens entry `name` (forward slashes, no leading slash).
    pub fn begin(&mut self, name: &str, method: Method) -> Result<()> {
        if self.open.is_some() {
            return Err(Error::Internal("zip: previous entry not ended".into()));
        }
        if name.is_empty() || name.starts_with('/') || name.contains('\\') || name.len() > u16::MAX as usize {
            return Err(Error::InvalidParams(format!("zip: bad entry name {name:?}")));
        }
        if self.entries.iter().any(|e| e.name == name) {
            return Err(Error::InvalidParams(format!("zip: duplicate entry {name:?}")));
        }
        if self.entries.len() >= u16::MAX as usize {
            return Err(Error::Io("zip: too many entries".into()));
        }
        let c = self.plain()?;
        let header_off = c.n;
        let mut h = Vec::with_capacity(30 + name.len());
        h.extend_from_slice(&LOCAL_SIG.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes()); // version needed: 2.0
        h.extend_from_slice(&FLAG_UTF8.to_le_bytes());
        h.extend_from_slice(&method.code().to_le_bytes());
        h.extend_from_slice(&DOS_TIME.to_le_bytes());
        h.extend_from_slice(&DOS_DATE.to_le_bytes());
        h.extend_from_slice(&[0u8; 12]); // crc, csize, usize: patched in end()
        h.extend_from_slice(&(name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(name.as_bytes());
        c.write_all(&h)?;
        let data_start = c.n;
        if method == Method::Deflate {
            let Sink::Plain(c) = std::mem::replace(&mut self.sink, Sink::Poisoned) else { unreachable!() };
            self.sink = Sink::Deflate(DeflateEncoder::new(c, Compression::new(6)));
        }
        self.open = Some(Open { name: name.into(), method, header_off, data_start, crc: crc32fast::Hasher::new(), size: 0 });
        Ok(())
    }

    /// Closes the open entry and patches its local header.
    pub fn end(&mut self) -> Result<()> {
        let open = self.open.take().ok_or_else(|| Error::Internal("zip: no entry open".into()))?;
        if let Sink::Deflate(_) = self.sink {
            let Sink::Deflate(enc) = std::mem::replace(&mut self.sink, Sink::Poisoned) else { unreachable!() };
            self.sink = Sink::Plain(enc.finish()?);
        }
        let c = self.plain()?;
        let csize = u32_of(c.n - open.data_start)?;
        let usize = u32_of(open.size)?;
        let offset = u32_of(open.header_off)?;
        let crc = open.crc.finalize();
        let end = c.w.stream_position()?;
        c.w.seek(SeekFrom::Start(open.header_off + 14))?;
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&crc.to_le_bytes());
        p[4..8].copy_from_slice(&csize.to_le_bytes());
        p[8..12].copy_from_slice(&usize.to_le_bytes());
        c.w.write_all(&p)?;
        c.w.seek(SeekFrom::Start(end))?;
        self.entries.push(CentralEntry { name: open.name, method: open.method, crc, csize, usize, offset });
        Ok(())
    }

    /// Adds a whole entry from memory.
    pub fn add(&mut self, name: &str, method: Method, bytes: &[u8]) -> Result<()> {
        self.begin(name, method)?;
        self.write_all(bytes)?;
        self.end()
    }

    /// Writes the central directory and returns the underlying writer (flushed).
    pub fn finish(mut self) -> Result<W> {
        if self.open.is_some() {
            return Err(Error::Internal("zip: entry still open at finish".into()));
        }
        let entries = std::mem::take(&mut self.entries);
        let c = self.plain()?;
        let cd_start = c.n;
        for e in &entries {
            let mut h = Vec::with_capacity(46 + e.name.len());
            h.extend_from_slice(&CENTRAL_SIG.to_le_bytes());
            h.extend_from_slice(&20u16.to_le_bytes()); // made by: 2.0, MS-DOS
            h.extend_from_slice(&20u16.to_le_bytes()); // needed
            h.extend_from_slice(&FLAG_UTF8.to_le_bytes());
            h.extend_from_slice(&e.method.code().to_le_bytes());
            h.extend_from_slice(&DOS_TIME.to_le_bytes());
            h.extend_from_slice(&DOS_DATE.to_le_bytes());
            h.extend_from_slice(&e.crc.to_le_bytes());
            h.extend_from_slice(&e.csize.to_le_bytes());
            h.extend_from_slice(&e.usize.to_le_bytes());
            h.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            h.extend_from_slice(&[0u8; 8]); // extra len, comment len, disk, internal attrs
            h.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            h.extend_from_slice(&e.offset.to_le_bytes());
            h.extend_from_slice(e.name.as_bytes());
            c.write_all(&h)?;
        }
        let cd_size = u32_of(c.n - cd_start)?;
        let cd_off = u32_of(cd_start)?;
        let n = entries.len() as u16;
        let mut h = Vec::with_capacity(22);
        h.extend_from_slice(&EOCD_SIG.to_le_bytes());
        h.extend_from_slice(&[0u8; 4]); // disk numbers
        h.extend_from_slice(&n.to_le_bytes());
        h.extend_from_slice(&n.to_le_bytes());
        h.extend_from_slice(&cd_size.to_le_bytes());
        h.extend_from_slice(&cd_off.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        c.write_all(&h)?;
        c.flush()?;
        let Sink::Plain(c) = std::mem::replace(&mut self.sink, Sink::Poisoned) else { unreachable!() };
        Ok(c.w)
    }
}

impl<W: Write + Seek> Write for ZipWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let Some(open) = self.open.as_mut() else {
            return Err(io::Error::other("zip: write with no entry open"));
        };
        let k = match &mut self.sink {
            Sink::Plain(c) => c.write(buf)?,
            Sink::Deflate(e) => e.write(buf)?,
            Sink::Poisoned => return Err(io::Error::other("zip: writer poisoned")),
        };
        open.crc.update(&buf[..k]);
        open.size += k as u64;
        Ok(k)
    }
    fn flush(&mut self) -> io::Result<()> {
        match &mut self.sink {
            Sink::Plain(c) => c.flush(),
            Sink::Deflate(e) => e.flush(),
            Sink::Poisoned => Ok(()),
        }
    }
}

/// One central-directory record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    pub name: String,
    pub method: u16,
    pub flags: u16,
    pub crc: u32,
    pub compressed_size: u64,
    pub size: u64,
    pub local_offset: u64,
}

/// ZIP reader over any seekable source.
pub struct ZipArchive<R: Read + Seek> {
    r: R,
    entries: Vec<EntryInfo>,
}

fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn bad(msg: impl Into<String>) -> Error {
    Error::Io(format!("zip: {}", msg.into()))
}

impl<R: Read + Seek> ZipArchive<R> {
    pub fn new(mut r: R) -> Result<Self> {
        let len = r.seek(SeekFrom::End(0))?;
        if len < 22 {
            return Err(bad("file too small to be a zip archive"));
        }
        let tail = len.min(22 + u16::MAX as u64);
        r.seek(SeekFrom::Start(len - tail))?;
        let mut buf = vec![0u8; tail as usize];
        r.read_exact(&mut buf)?;
        let pos = (0..=buf.len() - 22)
            .rev()
            .find(|&i| le32(&buf, i) == EOCD_SIG)
            .ok_or_else(|| bad("end of central directory not found (not a zip archive)"))?;
        let e = &buf[pos..];
        let n = le16(e, 10) as usize;
        let cd_size = le32(e, 12) as u64;
        let cd_off = le32(e, 16) as u64;
        if n == 0xFFFF || cd_off == 0xFFFF_FFFF {
            return Err(bad("ZIP64 archives are not supported"));
        }
        if cd_off + cd_size > len {
            return Err(bad("central directory out of range"));
        }
        r.seek(SeekFrom::Start(cd_off))?;
        let mut cd = vec![0u8; cd_size as usize];
        r.read_exact(&mut cd)?;
        let mut entries = Vec::with_capacity(n);
        let mut o = 0usize;
        for _ in 0..n {
            if o + 46 > cd.len() || le32(&cd, o) != CENTRAL_SIG {
                return Err(bad("corrupt central directory"));
            }
            let flags = le16(&cd, o + 8);
            let method = le16(&cd, o + 10);
            let crc = le32(&cd, o + 16);
            let csize = le32(&cd, o + 20) as u64;
            let usize = le32(&cd, o + 24) as u64;
            let nl = le16(&cd, o + 28) as usize;
            let xl = le16(&cd, o + 30) as usize;
            let cl = le16(&cd, o + 32) as usize;
            let off = le32(&cd, o + 42) as u64;
            if o + 46 + nl > cd.len() {
                return Err(bad("corrupt central directory name"));
            }
            let name = String::from_utf8_lossy(&cd[o + 46..o + 46 + nl]).into_owned();
            entries.push(EntryInfo { name, method, flags, crc, compressed_size: csize, size: usize, local_offset: off });
            o += 46 + nl + xl + cl;
        }
        Ok(ZipArchive { r, entries })
    }

    pub fn entries(&self) -> &[EntryInfo] {
        &self.entries
    }

    pub fn find(&self, name: &str) -> Option<&EntryInfo> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Streams an entry's uncompressed bytes. The reader returns an error at the
    /// end if the CRC or size does not match.
    pub fn open(&mut self, name: &str) -> Result<Box<dyn Read + '_>> {
        let e = self.find(name).cloned().ok_or_else(|| bad(format!("missing entry {name:?}")))?;
        if e.flags & 1 != 0 {
            return Err(bad(format!("{name}: encrypted entries are not supported")));
        }
        self.r.seek(SeekFrom::Start(e.local_offset))?;
        let mut h = [0u8; 30];
        self.r.read_exact(&mut h)?;
        if le32(&h, 0) != LOCAL_SIG {
            return Err(bad(format!("{name}: bad local header")));
        }
        let skip = le16(&h, 26) as i64 + le16(&h, 28) as i64;
        self.r.seek(SeekFrom::Current(skip))?;
        let raw = (&mut self.r).take(e.compressed_size);
        let inner: Box<dyn Read + '_> = match e.method {
            0 => Box::new(raw),
            8 => Box::new(DeflateDecoder::new(raw)),
            m => return Err(bad(format!("{name}: compression method {m} not supported"))),
        };
        Ok(Box::new(CrcReader { inner, crc: crc32fast::Hasher::new(), n: 0, want_crc: e.crc, want_n: e.size, done: false }))
    }

    /// Reads a whole entry, refusing entries larger than `max_bytes`.
    pub fn read(&mut self, name: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let size = self.find(name).map(|e| e.size).ok_or_else(|| bad(format!("missing entry {name:?}")))?;
        if size > max_bytes {
            return Err(bad(format!("{name}: {size} bytes exceeds the {max_bytes}-byte limit")));
        }
        let mut out = Vec::with_capacity(size as usize);
        self.open(name)?.take(max_bytes + 1).read_to_end(&mut out)?;
        Ok(out)
    }
}

struct CrcReader<'a> {
    inner: Box<dyn Read + 'a>,
    crc: crc32fast::Hasher,
    n: u64,
    want_crc: u32,
    want_n: u64,
    done: bool,
}

impl Read for CrcReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let k = self.inner.read(buf)?;
        self.crc.update(&buf[..k]);
        self.n += k as u64;
        if self.n > self.want_n {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "zip: entry longer than declared"));
        }
        if k == 0 && !buf.is_empty() && !self.done {
            self.done = true;
            if self.n != self.want_n || self.crc.clone().finalize() != self.want_crc {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "zip: CRC or size mismatch"));
            }
        }
        Ok(k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip_stored_and_deflate() {
        let mut z = ZipWriter::new(Cursor::new(Vec::new())).unwrap();
        z.add("a.txt", Method::Stored, b"hello").unwrap();
        z.begin("dir/b.bin", Method::Deflate).unwrap();
        for i in 0..10_000u32 {
            z.write_all(&i.to_le_bytes()).unwrap();
        }
        z.end().unwrap();
        z.add("empty", Method::Deflate, b"").unwrap();
        let bytes = z.finish().unwrap().into_inner();
        let mut a = ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(a.entries().len(), 3);
        assert_eq!(a.read("a.txt", 100).unwrap(), b"hello");
        let b = a.read("dir/b.bin", 1 << 20).unwrap();
        assert_eq!(b.len(), 40_000);
        assert_eq!(&b[4..8], &1u32.to_le_bytes());
        assert!(a.read("empty", 10).unwrap().is_empty());
        assert!(a.read("dir/b.bin", 100).is_err(), "size limit");
        assert!(a.read("nope", 100).is_err());
    }

    #[test]
    fn deterministic_bytes() {
        let build = || {
            let mut z = ZipWriter::new(Cursor::new(Vec::new())).unwrap();
            z.add("x", Method::Deflate, b"same content").unwrap();
            z.finish().unwrap().into_inner()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn corrupt_crc_is_detected() {
        let mut z = ZipWriter::new(Cursor::new(Vec::new())).unwrap();
        z.add("a", Method::Stored, b"abcdef").unwrap();
        let mut bytes = z.finish().unwrap().into_inner();
        bytes[30 + 1 + 2] ^= 0xFF; // flip a data byte ("c")
        let mut a = ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(a.read("a", 100).is_err());
    }

    #[test]
    fn rejects_duplicates_and_garbage() {
        let mut z = ZipWriter::new(Cursor::new(Vec::new())).unwrap();
        z.add("a", Method::Stored, b"1").unwrap();
        assert!(z.add("a", Method::Stored, b"2").is_err());
        assert!(ZipArchive::new(Cursor::new(vec![0u8; 100])).is_err());
    }
}
