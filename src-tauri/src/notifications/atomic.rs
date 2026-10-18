use super::{
    config::Config,
    error::{Code, Error, Result},
};
use std::{io::Write, path::Path};
pub(super) fn save(path: &Path, config: &Config) -> Result<()> {
    std::fs::create_dir_all(path.parent().ok_or(Code::Storage)?)
        .map_err(|_| Error::new(Code::Storage))?;
    let temporary = path.with_extension("json.tmp");
    let result = (|| {
        let mut file = std::fs::File::create(&temporary).map_err(|_| Error::new(Code::Storage))?;
        file.write_all(&serde_json::to_vec_pretty(config).map_err(|_| Error::new(Code::Storage))?)
            .map_err(|_| Error::new(Code::Storage))?;
        file.sync_all().map_err(|_| Error::new(Code::Storage))?;
        drop(file);
        replace(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
#[cfg(not(target_os = "windows"))]
fn replace(source: &Path, target: &Path) -> Result<()> {
    std::fs::rename(source, target).map_err(|_| Error::new(Code::Storage))
}
#[cfg(target_os = "windows")]
fn replace(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{ReplaceFileW, REPLACE_FILE_FLAGS},
    };
    if !target.exists() {
        return std::fs::rename(source, target).map_err(|_| Error::new(Code::Storage));
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        ReplaceFileW(
            PCWSTR(target.as_ptr()),
            PCWSTR(source.as_ptr()),
            PCWSTR::null(),
            REPLACE_FILE_FLAGS(0),
            None,
            None,
        )
    }
    .map_err(|_| Error::new(Code::Storage))
}
