#!/usr/bin/env python3
"""Static Linux payload audit. Never load or execute payload code (including ldd)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import struct
import tempfile

# Deliberately finite: AppImage's host graphics/audio stack and baseline ABI.
# https://github.com/AppImage/pkg2appimage/blob/master/excludelist
HOST_LIBRARIES = set("""
ld-linux-x86-64.so.2 libanl.so.1 libBrokenLocale.so.1 libc.so.6 libdl.so.2
libm.so.6 libmvec.so.1 libpthread.so.0 libresolv.so.2 librt.so.1
libthread_db.so.1 libutil.so.1 libstdc++.so.6 libgcc_s.so.1
libGL.so.1 libEGL.so.1 libGLdispatch.so.0 libGLX.so.0 libOpenGL.so.0
libdrm.so.2 libglapi.so.0 libgbm.so.1 libxcb.so.1 libX11.so.6
libX11-xcb.so.1 libwayland-client.so.0 libasound.so.2 libfontconfig.so.1
libfreetype.so.6 libharfbuzz.so.0 libcom_err.so.2 libexpat.so.1
libgpg-error.so.0 libICE.so.6 libSM.so.6 libusb-1.0.so.0 libuuid.so.1
libz.so.1 libjack.so.0 libpipewire-0.3.so.0
libfribidi.so.0 libgmp.so.10 libxcb-dri3.so.0 libxcb-dri2.so.0
""".split())
DEB_REQUIRED = {"libwebkit2gtk-4.1-0", "libgtk-3-0", "libssl3", "ca-certificates",
                "gstreamer1.0-plugins-base", "gstreamer1.0-plugins-good",
                "gstreamer1.0-plugins-bad", "gstreamer1.0-libav"}
# These direct dependencies are guaranteed by the declared GTK/WebKit packages.
DEB_LIBRARIES = HOST_LIBRARIES | set("""
libgdk-3.so.0 libgdk_pixbuf-2.0.so.0 libcairo.so.2 libgobject-2.0.so.0
libglib-2.0.so.0 libdbus-1.so.3 libwebkit2gtk-4.1.so.0 libgtk-3.so.0
libsoup-3.0.so.0 libgio-2.0.so.0 libjavascriptcoregtk-4.1.so.0
libssl.so.3 libcrypto.so.3
""".split())


def inspect_elf(path):
    result = subprocess.run(["readelf", "-W", "-h", "-l", "-d", "-V", str(path)],
                            capture_output=True, text=True, env={**os.environ, "LC_ALL": "C"})
    if result.returncode or result.stderr.strip():
        raise ValueError(f"invalid ELF: {result.stderr.strip()}")
    out = result.stdout
    field = lambda name: (re.search(r"^\s*" + name + r":\s*(.+)$", out, re.M).group(1).strip()
                          if re.search(r"^\s*" + name + r":\s*(.+)$", out, re.M) else None)
    tag = lambda name: re.findall(r"\(" + name + r"\).*?\[(.*?)\]", out)
    interpreter = re.search(r"Requesting program interpreter: (.*?)\]", out)
    needed_versions, definitions = {}, []
    section, library = None, None
    for line in out.splitlines():
        if line.startswith("Version needs section"):
            section = "needs"
        elif line.startswith("Version definition section"):
            section = "definitions"
        elif line.startswith("Version symbols section"):
            section = None
        if section == "needs":
            match = re.search(r"File: (\S+)", line)
            if match:
                library = match.group(1)
                needed_versions.setdefault(library, [])
            match = re.search(r"Name: (\S+)", line)
            if match and library:
                needed_versions[library].append(match.group(1))
        elif section == "definitions":
            match = re.search(r"Name: (\S+)", line)
            if match:
                definitions.append(match.group(1))
    return {"class": field("Class"), "machine": field("Machine"), "data": field("Data"),
            "type": field("Type"), "loadSegments": len(re.findall(r"^\s+LOAD\s", out, re.M)),
            "interpreter": interpreter.group(1) if interpreter else None,
            "needed": tag("NEEDED"), "soname": next(iter(tag("SONAME")), None),
            "rpath": tag("RPATH"), "runpath": tag("RUNPATH"),
            "versionNeeds": needed_versions, "versionDefinitions": definitions}


def safe_target(root, path):
    # Absolute payload links refer to the build machine, not the mounted AppDir.
    if path.is_symlink() and os.path.isabs(os.readlink(path)):
        raise ValueError("absolute symlink")
    resolved = path.resolve(strict=True)
    if not resolved.is_relative_to(root.resolve()):
        raise ValueError("symlink escapes payload")
    return resolved


def validate_metadata(root, records, kind):
    violations, missing, host = [], [], []
    directories = [root / "usr/lib", root / "usr/lib64",
                   root / "usr/lib/x86_64-linux-gnu", root / "lib",
                   root / "lib/x86_64-linux-gnu"]
    for name, record in records.items():
        path = root / name
        if record["class"] != "ELF64" or record["machine"] != "Advanced Micro Devices X86-64" or "little endian" not in (record["data"] or ""):
            violations.append(f"wrong ELF architecture: {name}")
        if record["interpreter"] and record["interpreter"] != "/lib64/ld-linux-x86-64.so.2":
            violations.append(f"unsupported interpreter: {name}: {record['interpreter']}")
        expanded_paths = []
        for value in record["rpath"] + record["runpath"]:
            for entry in value.split(":"):
                expanded = entry.replace("${ORIGIN}", str(path.parent)).replace("$ORIGIN", str(path.parent))
                candidate = Path(expanded)
                if not entry or "$" in expanded or not candidate.is_absolute() or not candidate.resolve().is_relative_to(root.resolve()):
                    violations.append(f"unsafe library search path: {name}: {entry}")
                else:
                    expanded_paths.append(candidate)
        # AppRun prepends bundle directories to LD_LIBRARY_PATH. No ambient host lookup.
        bundle_paths = directories if kind != "deb" else []
        search = (bundle_paths + expanded_paths if record["runpath"] else expanded_paths + bundle_paths)
        for needed in record["needed"]:
            provider = None
            for directory in search:
                candidate = directory / needed
                try:
                    target = safe_target(root, candidate)
                    provider = records.get(str(target.relative_to(root.resolve())))
                    if provider:
                        break
                except (ValueError, OSError, RuntimeError):
                    pass
            versions = record["versionNeeds"].get(needed, [])
            if provider:
                absent = sorted(set(versions) - set(provider["versionDefinitions"]))
                if absent:
                    violations.append(f"symbol version mismatch: {name} -> {needed}: {', '.join(absent)}")
            elif needed in (DEB_LIBRARIES if kind == "deb" else HOST_LIBRARIES):
                host.append({"consumer": name, "library": needed, "versions": versions})
            else:
                missing.append({"consumer": name, "library": needed})
            for version in versions:
                if re.fullmatch(r"GLIBC_[0-9.]+", version) and tuple(map(int, version[6:].split("."))) > (2, 35):
                    violations.append(f"exceeds Ubuntu 22.04 glibc baseline: {name}: {version}")
                for prefix, maximum in [("GLIBCXX_", (3, 4, 30)), ("CXXABI_", (1, 3, 13))]:
                    if re.fullmatch(prefix + r"[0-9.]+", version) and tuple(map(int, version[len(prefix):].split("."))) > maximum:
                        violations.append(f"exceeds Ubuntu 22.04 C++ baseline: {name}: {version}")
    violations += [f"missing dependency: {entry['consumer']} -> {entry['library']}" for entry in missing]
    return {"violations": sorted(set(violations)), "missingDependencies": missing, "hostDependencies": host}


def known_dynamic_dependencies(root, records):
    # NSS SRTP crypto uses a private softoken loader, not DT_NEEDED. Keep this
    # family together: a newer host softoken may not work with bundled NSS.
    # https://github.com/nss-dev/nss/blob/master/lib/pk11wrap/pk11load.c
    if not any(Path(name).name == "libnss3.so" or record["soname"] == "libnss3.so" for name, record in records.items()):
        return []
    result = []
    for consumer, library in [("libnss3.so", "libsoftokn3.so"), ("libsoftokn3.so", "libfreeblpriv3.so")]:
        try:
            target = safe_target(root, root / "usr/lib" / library)
            bundled = str(target.relative_to(root.resolve())) in records
        except (ValueError, OSError, RuntimeError):
            bundled = False
        result.append({"consumer": consumer, "library": library, "kind": "dlopen", "bundled": bundled})
    return result


def audit(root, kind, control=None):
    root = Path(root).resolve()
    records, links, violations, embedded = {}, [], [], []
    for directory, dirs, files in os.walk(root, followlinks=False):
        for name in dirs + files:
            path = Path(directory) / name
            relative = str(path.relative_to(root))
            if path.is_symlink():
                try:
                    target = safe_target(root, path)
                    links.append({"path": relative, "target": str(target.relative_to(root))})
                except (ValueError, OSError, RuntimeError) as error:
                    violations.append(f"invalid link: {relative}: {error}")
                continue
            if not path.is_file():
                continue
            with path.open("rb") as stream:
                magic = stream.read(4)
            if magic == b"\x7fELF":
                try:
                    records[relative] = inspect_elf(path)
                except ValueError as error:
                    violations.append(f"{relative}: {error}")
            elif re.search(r"\.so(?:\.[0-9]+)*$", path.name):
                violations.append(f"plugin is not ELF: {relative}")
    main = root / "usr/bin/meridian"
    if main.is_file():
        data = main.read_bytes()
        offset = 0
        while True:
            offset = data.find(b"\x7fELF", offset + 1)
            if offset < 0:
                break
            if data[offset + 4:offset + 7] != b"\x02\x01\x01" or offset + 64 > len(data):
                continue
            header = struct.unpack_from("<16sHHIQQQIHHHHHH", data, offset)
            size = header[6] + header[11] * header[12]
            if header[1] not in (2, 3) or header[2] != 62 or header[11] != 64 or not 64 < size <= len(data) - offset:
                continue
            payload = data[offset:offset + size]
            with tempfile.NamedTemporaryFile() as fixture:
                fixture.write(payload)
                fixture.flush()
                try:
                    metadata = inspect_elf(fixture.name)
                except ValueError:
                    continue
            embedded.append({"offset": offset, "size": size, "sha256": hashlib.sha256(payload).hexdigest(), "elf": metadata})
            records[f"usr/bin/meridian.embedded-{offset}"] = metadata
        if len(embedded) != 1:
            violations.append(f"expected one embedded runner ELF, found {len(embedded)}")
    required = ["usr/bin/meridian", "usr/bin/waybridge"]
    if kind != "deb":
        required += ["AppRun", "AppRun.wrapped", "usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner",
                     "usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess",
                     "usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitNetworkProcess"]
    for name in required:
        try:
            path = safe_target(root, root / name)
            if not path.is_file() or not path.stat().st_size or not path.stat().st_mode & 0o111:
                raise ValueError("empty or not executable")
            if name not in ("AppRun", "AppRun.wrapped") and str(path.relative_to(root)) not in records:
                raise ValueError("not a valid ELF executable")
            if name not in ("AppRun", "AppRun.wrapped"):
                record = records[str(path.relative_to(root))]
                if not record["loadSegments"] or not record["type"].startswith(("DYN", "EXEC")):
                    raise ValueError("ELF has no executable load segments")
        except (ValueError, OSError, RuntimeError) as error:
            violations.append(f"invalid executable: {name}: {error}")
    dependencies = validate_metadata(root, records, kind)
    dynamic = known_dynamic_dependencies(root, records) if kind != "deb" else []
    for dependency in dynamic:
        if not dependency["bundled"]:
            dependencies["missingDependencies"].append(dependency)
            violations.append(f"missing NSS dynamic module: {dependency['consumer']} -> {dependency['library']}")
    hooks = {}
    if kind != "deb":
        for name in ["apprun-hooks/linuxdeploy-plugin-gstreamer.sh", "apprun-hooks/linuxdeploy-plugin-gtk.sh"]:
            path = root / name
            if not path.is_file():
                violations.append(f"missing AppRun hook: {name}")
                continue
            hooks[name] = path.read_text()
        gst = hooks.get("apprun-hooks/linuxdeploy-plugin-gstreamer.sh", "")
        for variable, suffix in {
            "GST_PLUGIN_SYSTEM_PATH_1_0": "/usr/lib/gstreamer-1.0",
            "GST_PLUGIN_PATH_1_0": "/usr/lib/gstreamer-1.0",
            "GST_PLUGIN_SCANNER_1_0": "/usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner",
        }.items():
            if not re.search(r"^export " + variable + r'=\"\$\{APPDIR\}' + re.escape(suffix) + r'\"\s*$', gst, re.M):
                violations.append(f"incorrect media hook path: {variable}")
        apprun = (root / "AppRun").read_text(errors="replace") if (root / "AppRun").is_file() else ""
        wrapped = root / "AppRun.wrapped"
        loader = wrapped.read_bytes() if wrapped.is_file() else b""
        if "apprun-hooks" not in apprun or "AppRun.wrapped" not in apprun or b"LD_LIBRARY_PATH" not in loader or b"/usr/lib" not in loader:
            violations.append("AppRun does not load hooks and bundle library paths")
        for name in ["usr/lib/gio/modules/libgiognutls.so", "usr/lib/gio/modules/giomodule.cache"]:
            if not (root / name).is_file() or not (root / name).stat().st_size:
                violations.append(f"missing GIO TLS module/cache: {name}")
        for name in ["usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/injected-bundle/libwebkit2gtkinjectedbundle.so"]:
            if name not in records:
                violations.append(f"missing WebKit injected bundle: {name}")
        gtk = hooks.get("apprun-hooks/linuxdeploy-plugin-gtk.sh", "")
        for variable, suffix in {
            "GIO_MODULE_DIR": "usr/lib/gio/modules",
            "GSETTINGS_SCHEMA_DIR": "usr/share/glib-2.0/schemas",
            "GTK_IM_MODULE_FILE": "usr/lib/gtk-3.0/3.0.0/immodules.cache",
            "GDK_PIXBUF_MODULE_FILE": "usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache",
        }.items():
            if not re.search(r'^export ' + variable + r'="\$APPDIR/+' + re.escape(suffix) + r'"', gtk, re.M):
                violations.append(f"incorrect GTK hook path: {variable}")
            if not (root / suffix).exists():
                violations.append(f"missing GTK hook target: {suffix}")
        for cache, subdir in {
            "usr/lib/gio/modules/giomodule.cache": "",
            "usr/lib/gtk-3.0/3.0.0/immodules.cache": "immodules",
            "usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache": "loaders",
        }.items():
            path = root / cache
            if not path.is_file():
                violations.append(f"missing module cache: {cache}")
                continue
            modules = re.findall(r'^(?:"([^"\n]+\.so)"\s*$|([^:\n]+\.so):)', path.read_text(), re.M)
            if cache.endswith("giomodule.cache") and "gio-tls-backend" not in path.read_text():
                violations.append("GIO cache does not register TLS backend")
            for quoted, plain in modules:
                module = quoted or plain
                try:
                    target = safe_target(root, path.parent / subdir / module)
                    if str(target.relative_to(root)) not in records:
                        raise ValueError("module not ELF")
                except (ValueError, OSError, RuntimeError) as error:
                    violations.append(f"invalid cached module: {cache} -> {module}: {error}")
    else:
        packages = set(re.findall(r"(?:^|[,|])\s*([a-z0-9.+-]+)", (control or {}).get("Depends", "")))
        for package in sorted(DEB_REQUIRED - packages):
            violations.append(f"DEB does not declare runtime dependency: {package}")
        if (control or {}).get("Architecture") != "amd64":
            violations.append("DEB architecture must be amd64")
    certificate_paths = {}
    for name in ["usr/lib/libgnutls.so.30", "usr/lib/libcrypto.so.3"]:
        path = root / name
        if path.is_file():
            data = path.read_bytes()
            certificate_paths[name] = [value for value in ["/etc/ssl/certs", "/etc/ssl/certs/ca-certificates.crt", "/usr/lib/ssl"]
                                       if value.encode() in data]
    return {"elfFiles": records, "embeddedRunners": embedded, "links": links, "hooks": hooks,
            "knownDynamicDependencies": dynamic, **dependencies,
            "violations": sorted({error.replace(str(root), "<payload>") for error in violations + dependencies["violations"]}),
            "compiledCertificatePaths": certificate_paths,
            "certificatePolicy": "host CA trust store required; ca-certificates on Ubuntu, ca-certificates-utils on Arch"}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("root")
    parser.add_argument("kind", choices=["deb", "appimage", "appdir"])
    parser.add_argument("--control")
    args = parser.parse_args()
    print(json.dumps(audit(args.root, args.kind, json.loads(args.control) if args.control else None)))
