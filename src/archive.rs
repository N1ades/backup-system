use crate::project::BackupPlan;
use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

pub fn write_zip(plan: &BackupPlan, dest: &Path) -> Result<Vec<String>> {
    let file = File::create(dest)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut warnings = Vec::new();

    for pf in &plan.files {
        let name = pf.relative.to_string_lossy().replace('\\', "/");
        // A file present during scan can still vanish or become unreadable
        // by the time we archive it (deleted mid-run, permission change,
        // race with the source app). That should skip the one file, not
        // abort the whole backup.
        let opened = File::open(&pf.absolute).map(BufReader::new);
        let mut reader = match opened {
            Ok(r) => r,
            Err(e) => {
                warnings.push(format!("could not read {} ({e}), skipped", pf.relative.display()));
                continue;
            }
        };
        if let Err(e) = zip.start_file(&name, options) {
            warnings.push(format!("could not add {} to archive ({e}), skipped", pf.relative.display()));
            continue;
        }
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    warnings.push(format!("read error on {} ({e}), file may be truncated in archive", pf.relative.display()));
                    break;
                }
            };
            zip.write_all(&buf[..n])?;
        }
    }

    zip.finish()?;
    Ok(warnings)
}