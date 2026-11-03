use anyhow::{Context, Result};
use apple_dmg::DmgWriter;
use fatfs::{Dir, FileSystem, FormatVolumeOptions, FsOptions, ReadWriteSeek};
use fscommon::BufStream;
use std::{
    env, fs,
    fs::File,
    io::{copy, Cursor},
    path::Path,
};

const FAT32_SYMLINK_BYTES: usize = 1067;

fn dir_size(path: &Path) -> Result<u64> {
    let mut size = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            size += dir_size(&entry.path())?;
        } else if ty.is_file() {
            size += entry.metadata()?.len();
        } else if ty.is_symlink() {
            size += FAT32_SYMLINK_BYTES as u64;
        }
    }
    Ok(size)
}

fn symlink_payload(target: &str) -> Result<Vec<u8>> {
    let xsym = format!(
        "XSym\n{:04}\n{:x}\n{}\n",
        target.as_bytes().len(),
        md5::compute(target.as_bytes()),
        target,
    );
    let mut xsym = xsym.into_bytes();
    anyhow::ensure!(xsym.len() <= FAT32_SYMLINK_BYTES);
    xsym.resize(FAT32_SYMLINK_BYTES, b' ');
    Ok(xsym)
}

fn copy_dir<T: ReadWriteSeek>(src: &Path, dest: &Dir<'_, T>) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().context("non-utf8 file name")?;
        let source = entry.path();
        let ty = entry.file_type()?;

        if ty.is_dir() {
            let child = dest.create_dir(name)?;
            copy_dir(&source, &child)?;
        } else if ty.is_file() {
            let mut out = dest.create_file(name)?;
            let mut input = File::open(&source)?;
            copy(&mut input, &mut out)?;
        } else if ty.is_symlink() {
            let target = fs::read_link(&source)?;
            let target = target.to_str().context("non-utf8 symlink target")?;
            let payload = symlink_payload(target)?;
            let mut out = dest.create_file(name)?;
            copy(&mut &payload[..], &mut out)?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    anyhow::ensure!(
        args.len() == 4,
        "usage: linux-udif-maker <src-dir> <out.dmg> <volume-label>"
    );

    let src = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    let label = &args[3];
    let bytes = dir_size(src)?;
    let reserve = 96u64 * 1024 * 1024;
    let sectors = ((bytes + reserve + 511) / 512).max(262_144) as u32;
    let mut fat32 = vec![0; sectors as usize * 512];

    {
        let mut label_bytes = [b' '; 11];
        let ascii = label.as_bytes();
        let n = ascii.len().min(11);
        label_bytes[..n].copy_from_slice(&ascii[..n]);

        let opts = FormatVolumeOptions::new()
            .volume_label(label_bytes)
            .bytes_per_sector(512)
            .total_sectors(sectors);
        let mut disk = BufStream::new(Cursor::new(&mut fat32));
        fatfs::format_volume(&mut disk, opts)?;
        let fs = FileSystem::new(disk, FsOptions::new())?;
        copy_dir(src, &fs.root_dir())?;
    }

    if out.exists() {
        fs::remove_file(out)?;
    }
    DmgWriter::create(out)?.create_fat32(&fat32)?;
    Ok(())
}
