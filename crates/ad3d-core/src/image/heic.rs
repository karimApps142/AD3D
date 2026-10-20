//! HEIC/HEIF import on macOS through the system's `/usr/bin/sips` (design §16 step 1).
//!
//! ISOLATED IO HELPER: this spawns a process and writes a temporary file, so only the
//! app crate (or CLI) may call it, never a generator. It converts the file to a
//! temporary PNG, decodes that with [`decode_bytes`](super::decode_bytes) (which
//! applies any orientation tag sips carried over) and deletes the temporary file.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use super::LinearLuma;
use crate::error::{Error, Result};

const SIPS: &str = "/usr/bin/sips";

/// True for file names the `image` crate cannot decode but sips can (HEIC/HEIF).
pub fn needs_sips(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("heic" | "heif" | "hif")
    )
}

fn temp_png() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    std::env::temp_dir().join(format!(
        "ad3d-sips-{}-{}-{}.png",
        std::process::id(),
        nanos,
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Removes the temporary file on every exit path.
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Converts any image sips can read (HEIC, HEIF, …) to a temporary PNG and decodes it.
pub fn decode_with_sips(path: &Path) -> Result<LinearLuma> {
    if !path.is_file() {
        return Err(Error::Io(format!("{}: no such file", path.display())));
    }
    let tmp = TempFile(temp_png());
    let out = Command::new(SIPS)
        .arg("-s")
        .arg("format")
        .arg("png")
        .arg(path)
        .arg("--out")
        .arg(&tmp.0)
        .output()
        .map_err(|e| Error::Io(format!("{SIPS}: {e}")))?;
    if !out.status.success() || !tmp.0.is_file() {
        return Err(Error::Io(format!(
            "{}: sips could not convert it ({})",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    LinearLuma::load(&tmp.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_check() {
        assert!(needs_sips(Path::new("a/IMG_1.HEIC")));
        assert!(needs_sips(Path::new("x.heif")));
        assert!(!needs_sips(Path::new("x.png")));
    }

    #[test]
    fn missing_file_is_io_error() {
        assert!(matches!(decode_with_sips(Path::new("/nonexistent/x.heic")), Err(Error::Io(_))));
    }

    /// Round-trips a PNG through sips (and through HEIC when this macOS can encode it).
    #[test]
    fn sips_round_trip() {
        if !Path::new(SIPS).exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("ad3d-heic-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("in.png");
        let img = image::GrayImage::from_fn(8, 4, |x, _| image::Luma([(x * 32) as u8]));
        img.save(&png).unwrap();
        let l = decode_with_sips(&png).unwrap();
        assert_eq!((l.w, l.h), (8, 4));
        assert!(l.px[7] > l.px[0]);
        let heic = dir.join("in.heic");
        let enc = Command::new(SIPS).args(["-s", "format", "heic"]).arg(&png).arg("--out").arg(&heic).output();
        if enc.map(|o| o.status.success()).unwrap_or(false) && heic.is_file() {
            let h = decode_with_sips(&heic).unwrap();
            assert_eq!((h.w, h.h), (8, 4));
            assert!(h.px[7] > h.px[0] + 0.3, "lossy but monotone: {:?}", &h.px[..8]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
