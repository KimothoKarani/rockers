use std::fs::File;
use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tar::Archive;

const LAYERS: [&str; 7] = [
    "0_sha256:a7730063fcfe708b222d34c4f07d633dfe087a28c05c4daaab2fa9943854c155.tar.gz",
    "1_sha256:f33970743aee750df25f6c661132b7401c8fefe930e5f4803f4c8b6f567a6b55.tar.gz",
    "2_sha256:5397da1d1537c4d725f3090c5688a582e14eeaf7743d75d9b38bad1649554987.tar.gz",
    "3_sha256:59c21dfbee0a20016b8b6053efb4c9a5743bb9373bbba6d9f2ee60ad9e53914a.tar.gz",
    "4_sha256:b55da06e3b41084804b2e5dbba71d35d26478b19ba8055d07a393cae22e9935f.tar.gz",
    "5_sha256:48e102291f75c96b251bde878d5163a895f883816bbf6de39810b683e3770bcd.tar.gz",
    "6_sha256:4f4fb700ef54461cfa02571ae0db9a0dc1e0cdb5577484a6d75e68dc38e8acc1.tar.gz",
];

const TARGET: &str = "./tmp/rootfs";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let progress = MultiProgress::new();
    let style = ProgressStyle::with_template(
        "{msg:<2} [{bar:40.green/white}] {bytes:>8}/{total_bytes:8} ({bytes_per_sec}, {eta})",
    )?
    .progress_chars("=>-");

    for l in LAYERS {
        let path = format!("./tmp/layers/{}", l);
        let digest_short = &l.split(':').last().unwrap()[7..7 + 12];
        let file = File::open(path)?;

        let bar = progress.add(ProgressBar::new(file.metadata()?.len()));
        bar.set_style(style.clone());
        bar.set_message(digest_short.to_owned());
        extract_tar_gz(bar.wrap_read(file), &TARGET)?;
        bar.finish_with_message(format!("{digest_short}: Download complete"));
    }
    Ok(())
}

fn extract_tar_gz(file: impl Read, target_path: impl AsRef<Path>) -> anyhow::Result<()> {
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive.unpack(target_path)?;
    Ok(())
}
