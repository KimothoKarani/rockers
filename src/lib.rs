use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use tar::Archive;

pub mod registry;

pub fn extract_tar_gz(file: impl Read, target_path: impl AsRef<Path>) -> anyhow::Result<()> {
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive.unpack(target_path)?;
    Ok(())
}
