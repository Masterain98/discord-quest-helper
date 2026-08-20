//! Rewrite VERSIONINFO and strip icons on a copied Windows executable.
//!
//! Applied only to the stealth temp copy. The installed product binary is
//! left unchanged.

use std::path::Path;

use windows::core::{BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, HANDLE, HMODULE};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::LibraryLoader::{
    BeginUpdateResourceW, EndUpdateResourceW, EnumResourceLanguagesW, EnumResourceNamesW,
    LoadLibraryExW, UpdateResourceW, LOAD_LIBRARY_AS_DATAFILE,
};
use windows::Win32::UI::WindowsAndMessaging::{RT_GROUP_ICON, RT_ICON, RT_VERSION};

const VERSION_LANG: u16 = 0x0409;
const STRING_TABLE_KEY: &str = "040904B0";

#[derive(Clone)]
enum ResourceId {
    Id(u16),
    Name(Vec<u16>),
}

impl ResourceId {
    fn from_pcwstr(value: PCWSTR) -> Self {
        let ptr = value.0 as usize;
        if ptr >> 16 == 0 {
            return Self::Id(ptr as u16);
        }
        let mut name = Vec::new();
        unsafe {
            let mut i = 0;
            loop {
                let ch = *value.0.add(i);
                name.push(ch);
                if ch == 0 {
                    break;
                }
                i += 1;
            }
        }
        Self::Name(name)
    }

    fn as_pcwstr(&self) -> PCWSTR {
        match self {
            Self::Id(id) => PCWSTR(*id as usize as *const u16),
            Self::Name(name) => PCWSTR(name.as_ptr()),
        }
    }
}

/// Replace version strings on `exe` so they match `file_name` / `stem`,
/// and delete icon resources.
pub fn rewrite_copy_identity(exe: &Path, file_name: &str, stem: &str) -> Result<(), String> {
    let blob = build_version_info(file_name, stem);
    let icons = collect_resources(exe, RT_ICON);
    let groups = collect_resources(exe, RT_GROUP_ICON);
    let versions = collect_resources(exe, RT_VERSION);

    let path = HSTRING::from(exe.to_string_lossy().as_ref());
    let update = unsafe { BeginUpdateResourceW(&path, false) }
        .map_err(|err| format!("BeginUpdateResourceW: {err}"))?;

    let result = (|| {
        delete_resources(update, RT_ICON, &icons)?;
        delete_resources(update, RT_GROUP_ICON, &groups)?;
        delete_resources(update, RT_VERSION, &versions)?;
        unsafe {
            UpdateResourceW(
                update,
                RT_VERSION,
                PCWSTR(1u16 as usize as *const u16),
                VERSION_LANG,
                Some(blob.as_ptr().cast()),
                blob.len() as u32,
            )
        }
        .map_err(|err| format!("UpdateResourceW RT_VERSION: {err}"))
    })();

    let discard = result.is_err();
    unsafe { EndUpdateResourceW(update, discard) }
        .map_err(|err| format!("EndUpdateResourceW: {err}"))?;
    result
}

fn delete_resources(
    update: HANDLE,
    resource_type: PCWSTR,
    resources: &[(ResourceId, u16)],
) -> Result<(), String> {
    for (name, lang) in resources {
        unsafe { UpdateResourceW(update, resource_type, name.as_pcwstr(), *lang, None, 0) }
            .map_err(|err| format!("UpdateResourceW delete: {err}"))?;
    }
    Ok(())
}

fn collect_resources(path: &Path, resource_type: PCWSTR) -> Vec<(ResourceId, u16)> {
    let mut out = Vec::new();
    let path = HSTRING::from(path.to_string_lossy().as_ref());
    let Ok(module) = (unsafe { LoadLibraryExW(&path, None, LOAD_LIBRARY_AS_DATAFILE) }) else {
        return out;
    };

    unsafe {
        let _ = EnumResourceNamesW(
            Some(module),
            resource_type,
            Some(enum_resource_names),
            &mut out as *mut Vec<(ResourceId, u16)> as isize,
        );
        let _ = FreeLibrary(module);
    }
    out
}

unsafe extern "system" fn enum_resource_names(
    module: HMODULE,
    lptype: PCWSTR,
    lpname: PCWSTR,
    lparam: isize,
) -> BOOL {
    let out = unsafe { &mut *(lparam as *mut Vec<(ResourceId, u16)>) };
    let name = ResourceId::from_pcwstr(lpname);
    let mut langs: Vec<u16> = Vec::new();
    let _ = unsafe {
        EnumResourceLanguagesW(
            Some(module),
            lptype,
            lpname,
            Some(enum_resource_langs),
            &mut langs as *mut Vec<u16> as isize,
        )
    };
    if langs.is_empty() {
        langs.push(VERSION_LANG);
        langs.push(0);
    }
    for lang in langs {
        out.push((name.clone(), lang));
    }
    true.into()
}

unsafe extern "system" fn enum_resource_langs(
    _module: HMODULE,
    _lptype: PCWSTR,
    _lpname: PCWSTR,
    wlanguage: u16,
    lparam: isize,
) -> BOOL {
    let langs = unsafe { &mut *(lparam as *mut Vec<u16>) };
    langs.push(wlanguage);
    true.into()
}

fn pad4(buf: &mut Vec<u8>) {
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
}

fn utf16_nul(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|unit| unit.to_le_bytes())
        .collect()
}

fn put_u16(buf: &mut Vec<u8>, value: u16) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn patch_length(buf: &mut [u8]) {
    let len = buf.len() as u16;
    buf[0..2].copy_from_slice(&len.to_le_bytes());
}

fn version_node(
    key: &str,
    w_type: u16,
    value_length: u16,
    value: &[u8],
    children: &[u8],
) -> Vec<u8> {
    let mut inner = Vec::new();
    put_u16(&mut inner, 0);
    put_u16(&mut inner, value_length);
    put_u16(&mut inner, w_type);
    inner.extend_from_slice(&utf16_nul(key));
    pad4(&mut inner);
    inner.extend_from_slice(value);
    if !value.is_empty() {
        pad4(&mut inner);
    }
    inner.extend_from_slice(children);
    patch_length(&mut inner);
    inner
}

fn string_entry(key: &str, value: &str) -> Vec<u8> {
    let value_bytes = utf16_nul(value);
    version_node(key, 1, (value_bytes.len() / 2) as u16, &value_bytes, &[])
}

pub(crate) fn build_version_info(file_name: &str, stem: &str) -> Vec<u8> {
    let mut strings = Vec::new();
    strings.extend_from_slice(&string_entry("CompanyName", stem));
    strings.extend_from_slice(&string_entry("FileDescription", stem));
    strings.extend_from_slice(&string_entry("FileVersion", "1.0.0.0"));
    strings.extend_from_slice(&string_entry("InternalName", stem));
    strings.extend_from_slice(&string_entry("LegalCopyright", ""));
    strings.extend_from_slice(&string_entry("OriginalFilename", file_name));
    strings.extend_from_slice(&string_entry("ProductName", stem));
    strings.extend_from_slice(&string_entry("ProductVersion", "1.0.0.0"));

    let string_table = version_node(STRING_TABLE_KEY, 1, 0, &[], &strings);
    let string_file_info = version_node("StringFileInfo", 1, 0, &[], &string_table);

    let mut translation = Vec::new();
    put_u32(&mut translation, 0x04B0_0409);
    let var = version_node(
        "Translation",
        0,
        translation.len() as u16,
        &translation,
        &[],
    );
    let var_file_info = version_node("VarFileInfo", 1, 0, &[], &var);

    let mut fixed = Vec::new();
    put_u32(&mut fixed, 0xFEEF_04BD); // dwSignature
    put_u32(&mut fixed, 0x0001_0000); // dwStrucVersion
    put_u32(&mut fixed, 0x0001_0000); // dwFileVersionMS 1.0
    put_u32(&mut fixed, 0x0000_0000); // dwFileVersionLS 0.0
    put_u32(&mut fixed, 0x0001_0000); // dwProductVersionMS
    put_u32(&mut fixed, 0x0000_0000); // dwProductVersionLS
    put_u32(&mut fixed, 0x0000_003F); // dwFileFlagsMask
    put_u32(&mut fixed, 0); // dwFileFlags
    put_u32(&mut fixed, 0x0004_0004); // dwFileOS VOS_NT_WINDOWS32
    put_u32(&mut fixed, 0x0000_0001); // dwFileType VFT_APP
    put_u32(&mut fixed, 0); // dwFileSubtype
    put_u32(&mut fixed, 0); // dwFileDateMS
    put_u32(&mut fixed, 0); // dwFileDateLS

    let mut children = Vec::new();
    children.extend_from_slice(&string_file_info);
    children.extend_from_slice(&var_file_info);

    version_node("VS_VERSION_INFO", 0, fixed.len() as u16, &fixed, &children)
}

pub(crate) fn query_version_string(path: &Path, key: &str) -> Result<String, String> {
    let path_h = HSTRING::from(path.to_string_lossy().as_ref());
    let mut handle = 0u32;
    let size = unsafe { GetFileVersionInfoSizeW(&path_h, Some(&mut handle)) };
    if size == 0 {
        return Err("GetFileVersionInfoSizeW returned 0".to_string());
    }
    let mut buffer = vec![0u8; size as usize];
    unsafe {
        GetFileVersionInfoW(&path_h, Some(handle), size, buffer.as_mut_ptr().cast())
            .map_err(|err| format!("GetFileVersionInfoW: {err}"))?;
    }

    let sub = HSTRING::from(format!("\\StringFileInfo\\{STRING_TABLE_KEY}\\{key}"));
    let mut value: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut len = 0u32;
    let ok = unsafe { VerQueryValueW(buffer.as_ptr().cast(), &sub, &mut value, &mut len) };
    if !bool::from(ok) || value.is_null() {
        return Err(format!("VerQueryValueW missing {key}"));
    }
    let text = unsafe { PCWSTR(value.cast()).to_string() }
        .map_err(|err| format!("version string utf16: {err}"))?;
    Ok(text.trim_end_matches('\0').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    #[test]
    fn version_blob_contains_stem_utf16() {
        let blob = build_version_info("c0ffee12beef.exe", "c0ffee12beef");
        let product = "c0ffee12beef"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect::<Vec<_>>();
        assert!(blob.windows(product.len()).any(|w| w == product));
        let leaked = "Discord Quest Helper"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect::<Vec<_>>();
        assert!(!blob.windows(leaked.len()).any(|w| w == leaked));
    }

    #[test]
    fn rewrite_replaces_version_strings_on_copied_exe() {
        let src = env::current_exe().expect("current_exe");
        let dir = env::temp_dir().join(format!(
            "stealth-pe-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("c0ffee12beef.exe");
        fs::copy(&src, &dest).expect("copy test exe");

        rewrite_copy_identity(&dest, "c0ffee12beef.exe", "c0ffee12beef")
            .expect("rewrite_copy_identity");

        let original = query_version_string(&dest, "OriginalFilename").expect("OriginalFilename");
        let product = query_version_string(&dest, "ProductName").expect("ProductName");
        let description = query_version_string(&dest, "FileDescription").expect("FileDescription");

        assert_eq!(original, "c0ffee12beef.exe");
        assert_eq!(product, "c0ffee12beef");
        assert_eq!(description, "c0ffee12beef");
        assert!(!product.contains("Discord Quest Helper"));

        let _ = fs::remove_dir_all(&dir);
    }
}
