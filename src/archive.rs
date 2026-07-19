use crate::project::BackupPlan;
use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

pub fn write_zip(plan: &BackupPlan, dest: &Path) -> Result<()> {
    let file = File::create(dest)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for pf in &plan.files {
        let name = pf.relative.to_string_lossy().replace('\\', "/");
        zip.start_file(name, options)?;
        let mut reader = BufReader::new(File::open(&pf.absolute)?);
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            zip.write_all(&buf[..n])?;
        }
    }

    zip.finish()?;
    Ok(())
}
