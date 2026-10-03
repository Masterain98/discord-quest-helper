//! Keep AppImage's private runtime out of programs provided by the desktop.
use std::ffi::{OsStr, OsString};
use std::process::Command;

fn components(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts
}

fn overrides(environment: &[(OsString, OsString)]) -> Vec<(OsString, Option<OsString>)> {
    let Some(appdir) = environment
        .iter()
        .find(|(key, _)| key == "APPDIR")
        .and_then(|(_, value)| value.to_str())
        .filter(|value| value.starts_with('/') && !components(value).is_empty())
    else {
        return Vec::new();
    };
    let base = components(appdir);
    let bundled = |value: &str| value.starts_with('/') && components(value).starts_with(&base);
    let mut result = Vec::new();
    let media_hook = environment.iter().any(|(key, value)| {
        key == "GST_PLUGIN_SYSTEM_PATH_1_0" && value.to_str().is_some_and(bundled)
    });
    for (key, value) in environment {
        let Some(name) = key.to_str() else { continue };
        if matches!(name, "APPDIR" | "APPIMAGE" | "ARGV0" | "APPIMAGE_GTK_THEME")
            || (media_hook && name == "GST_REGISTRY_REUSE_PLUGIN_SCANNER")
        {
            result.push((key.clone(), None));
            continue;
        }
        let Some(text) = value.to_str() else { continue };
        let list = matches!(
            name,
            "LD_LIBRARY_PATH"
                | "PATH"
                | "XDG_DATA_DIRS"
                | "GI_TYPELIB_PATH"
                | "GTK_PATH"
                | "GIO_EXTRA_MODULES"
                | "GST_PLUGIN_PATH"
                | "GST_PLUGIN_PATH_1_0"
                | "GST_PLUGIN_SYSTEM_PATH"
                | "GST_PLUGIN_SYSTEM_PATH_1_0"
        );
        let single = matches!(
            name,
            "GIO_MODULE_DIR"
                | "GSETTINGS_SCHEMA_DIR"
                | "GTK_EXE_PREFIX"
                | "GTK_DATA_PREFIX"
                | "GTK_IM_MODULE_FILE"
                | "GDK_PIXBUF_MODULE_FILE"
                | "GDK_PIXBUF_MODULEDIR"
                | "GST_PLUGIN_SCANNER"
                | "GST_PLUGIN_SCANNER_1_0"
                | "GST_PTP_HELPER"
                | "GST_PTP_HELPER_1_0"
                | "GST_REGISTRY"
                | "GST_REGISTRY_1_0"
        );
        if list && text.split(':').any(bundled) {
            let kept = text
                .split(':')
                .filter(|entry| !bundled(entry))
                .collect::<Vec<_>>()
                .join(":");
            result.push((
                key.clone(),
                (!kept.is_empty()).then(|| OsString::from(kept)),
            ));
        } else if single && bundled(text) {
            result.push((key.clone(), None));
        } else if name == "LD_PRELOAD" {
            let entries = text
                .split([':', ' '])
                .filter(|entry| !entry.is_empty())
                .collect::<Vec<_>>();
            if entries.iter().any(|entry| bundled(entry)) {
                let kept = entries
                    .into_iter()
                    .filter(|entry| !bundled(entry))
                    .collect::<Vec<_>>()
                    .join(" ");
                result.push((
                    key.clone(),
                    (!kept.is_empty()).then(|| OsString::from(kept)),
                ));
            }
        } else if name == "GTK_THEME" && matches!(text, "Adwaita:light" | "Adwaita:dark") {
            // linuxdeploy's GTK hook sets this for the private GTK installation.
            result.push((key.clone(), None));
        }
    }
    result
}

pub fn linux_host_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    for (key, value) in overrides(&std::env::vars_os().collect::<Vec<_>>()) {
        if let Some(value) = value {
            command.env(key, value);
        } else {
            command.env_remove(key);
        }
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    fn changes(input: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
        overrides(
            &input
                .iter()
                .map(|(key, value)| (OsString::from(key), OsString::from(value)))
                .collect::<Vec<_>>(),
        )
        .into_iter()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect()
    }
    #[test]
    fn removes_only_private_runtime_paths() {
        assert_eq!(
            changes(&[
                ("APPDIR", "/tmp/.mount_app"),
                ("LD_LIBRARY_PATH", "/tmp/.mount_app//usr/lib:/opt/host/lib"),
                ("XDG_DATA_DIRS", "/tmp/.mount_app/usr/share:/usr/share"),
                ("PATH", "/tmp/.mount_app/usr/bin:/usr/bin"),
                ("GIO_MODULE_DIR", "/tmp/.mount_app/usr/lib/gio/modules"),
                ("GST_PLUGIN_PATH_1_0", "/opt/host/plugins"),
                ("DISPLAY", ":0"),
                ("WAYLAND_DISPLAY", "wayland-0"),
                ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
                ("XDG_RUNTIME_DIR", "/run/user/1000"),
                ("HTTPS_PROXY", "http://localhost:7890")
            ]),
            vec![
                ("APPDIR".into(), None),
                ("LD_LIBRARY_PATH".into(), Some("/opt/host/lib".into())),
                ("XDG_DATA_DIRS".into(), Some("/usr/share".into())),
                ("PATH".into(), Some("/usr/bin".into())),
                ("GIO_MODULE_DIR".into(), None)
            ]
        );
    }
    #[test]
    fn preserves_normal_environment_and_adjacent_paths() {
        assert!(changes(&[("LD_LIBRARY_PATH", "/usr/lib")]).is_empty());
        assert_eq!(
            changes(&[
                ("APPDIR", "/tmp/app"),
                ("LD_LIBRARY_PATH", "/tmp/application/lib")
            ]),
            vec![("APPDIR".into(), None)]
        );
        assert!(changes(&[("APPDIR", "relative/path"), ("GTK_THEME", "Adwaita:dark")]).is_empty());
    }
    #[test]
    fn removes_scanner_and_private_preload_but_keeps_host_preload() {
        assert_eq!(
            changes(&[
                ("APPDIR", "/tmp/app"),
                (
                    "GST_PLUGIN_SYSTEM_PATH_1_0",
                    "/tmp/app/usr/lib/gstreamer-1.0"
                ),
                ("GST_REGISTRY_REUSE_PLUGIN_SCANNER", "no"),
                ("GST_PLUGIN_SCANNER_1_0", "/tmp/app/usr/lib/scanner"),
                ("LD_PRELOAD", "/tmp/app/lib/private.so:/opt/host.so")
            ]),
            vec![
                ("APPDIR".into(), None),
                ("GST_PLUGIN_SYSTEM_PATH_1_0".into(), None),
                ("GST_REGISTRY_REUSE_PLUGIN_SCANNER".into(), None),
                ("GST_PLUGIN_SCANNER_1_0".into(), None),
                ("LD_PRELOAD".into(), Some("/opt/host.so".into()))
            ]
        );
    }
    #[test]
    fn building_a_command_does_not_launch_it() {
        let command = linux_host_command("fixture-not-executed");
        assert_eq!(command.get_program(), "fixture-not-executed");
    }
}
