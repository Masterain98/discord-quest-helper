"""Synthetic metadata/files only. No compiler, loader, display or app execution."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("audit_linux_elf", Path(__file__).with_name("audit-linux-elf.py"))
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)


def metadata(**changes):
    return {"class": "ELF64", "machine": "Advanced Micro Devices X86-64",
            "data": "2's complement, little endian", "interpreter": None,
            "needed": [], "soname": None, "rpath": [], "runpath": [],
            "versionNeeds": {}, "versionDefinitions": [], **changes}


class LinuxAuditTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "usr/lib/plugins").mkdir(parents=True)

    def check(self, records, kind="appdir"):
        return policy.validate_metadata(self.root, records, kind)

    def provider(self, name):
        path = self.root / "usr/lib" / name
        path.write_bytes(b"synthetic provider")
        return f"usr/lib/{name}"

    def test_architecture(self):
        result = self.check({"usr/lib/plugin.so": metadata(machine="AArch64")})
        self.assertIn("wrong ELF architecture", result["violations"][0])

    def test_interpreter(self):
        result = self.check({"usr/bin/app": metadata(interpreter="/lib/ld-musl-x86_64.so.1")})
        self.assertIn("unsupported interpreter", result["violations"][0])

    def test_host_dependencies_are_explicit(self):
        result = self.check({"usr/bin/app": metadata(needed=["libc.so.6", "libwayland-client.so.0"])})
        self.assertEqual(result["violations"], [])
        self.assertEqual(len(result["hostDependencies"]), 2)

    def test_unknown_dependency_is_rejected(self):
        result = self.check({"usr/lib/plugin.so": metadata(needed=["libuninstalled.so.1"])})
        self.assertEqual(result["missingDependencies"], [{"consumer": "usr/lib/plugin.so", "library": "libuninstalled.so.1"}])

    def test_transitive_dependencies_are_checked(self):
        provider = self.provider("libfirst.so.1")
        result = self.check({"usr/lib/plugins/plugin.so": metadata(needed=["libfirst.so.1"]),
                             provider: metadata(needed=["libsecond.so.1"])})
        self.assertEqual(result["missingDependencies"], [{"consumer": provider, "library": "libsecond.so.1"}])

    def test_symbol_version_mismatch(self):
        provider = self.provider("libfirst.so.1")
        result = self.check({"usr/lib/plugins/plugin.so": metadata(needed=["libfirst.so.1"], versionNeeds={"libfirst.so.1": ["FIRST_2"]}),
                             provider: metadata(versionDefinitions=["FIRST_1"])})
        self.assertIn("symbol version mismatch", result["violations"][0])

    def test_matching_symbol_version(self):
        provider = self.provider("libfirst.so.1")
        result = self.check({"usr/lib/plugins/plugin.so": metadata(needed=["libfirst.so.1"], versionNeeds={"libfirst.so.1": ["FIRST_2"]}),
                             provider: metadata(versionDefinitions=["FIRST_2"])})
        self.assertEqual(result["violations"], [])

    def test_glibc_baseline(self):
        result = self.check({"usr/bin/app": metadata(needed=["libc.so.6"], versionNeeds={"libc.so.6": ["GLIBC_2.36"]})})
        self.assertIn("glibc baseline", result["violations"][0])

    def test_origin_runpath(self):
        provider = self.provider("libfirst.so.1")
        result = self.check({"usr/lib/plugins/plugin.so": metadata(needed=["libfirst.so.1"], runpath=["$ORIGIN/.."]),
                             provider: metadata()})
        self.assertEqual(result["violations"], [])

    def test_unsafe_runpaths(self):
        for runpath in ["/usr/lib", "", "$ORIGIN/../../../../outside", ".", "$LIB"]:
            with self.subTest(runpath=runpath):
                self.assertTrue(self.check({"usr/lib/plugin.so": metadata(runpath=[runpath])})["violations"])

    def test_internal_link(self):
        self.provider("libfirst.so.1.0")
        link = self.root / "usr/lib/libfirst.so.1"
        link.symlink_to("libfirst.so.1.0")
        self.assertEqual(policy.safe_target(self.root, link), link.parent / "libfirst.so.1.0")
        result = self.check({"usr/lib/plugin.so": metadata(needed=["libfirst.so.1"]),
                             "usr/lib/libfirst.so.1.0": metadata()})
        self.assertEqual(result["violations"], [])

    def test_broken_link(self):
        link = self.root / "broken"
        link.symlink_to("missing")
        with self.assertRaises(OSError):
            policy.safe_target(self.root, link)

    def test_escape_link(self):
        link = self.root / "escape"
        link.symlink_to("..")
        with self.assertRaises(ValueError):
            policy.safe_target(self.root, link)

    def test_absolute_link(self):
        link = self.root / "absolute"
        link.symlink_to(self.root / "usr")
        with self.assertRaises(ValueError):
            policy.safe_target(self.root, link)

    def test_deb_required_packages(self):
        result = policy.audit(self.root, "deb", {"Architecture": "amd64", "Depends": "libgtk-3-0, libwebkit2gtk-4.1-0"})
        self.assertTrue(any("gstreamer1.0-libav" in error for error in result["violations"]))
        result = policy.audit(self.root, "deb", {"Architecture": "amd64", "Depends": ", ".join(sorted(policy.DEB_REQUIRED))})
        self.assertFalse(any("runtime dependency" in error for error in result["violations"]))

    def test_nss_dynamic_modules_are_required_only_with_bundled_nss(self):
        self.assertEqual(policy.known_dynamic_dependencies(self.root, {}), [])
        missing = policy.known_dynamic_dependencies(self.root, {"usr/lib/libnss3.so": metadata()})
        self.assertEqual([entry["library"] for entry in missing if not entry["bundled"]], ["libsoftokn3.so", "libfreeblpriv3.so"])

    def test_nss_internal_module_link_is_accepted(self):
        softoken = self.provider("libsoftokn3.so.1")
        (self.root / "usr/lib/libsoftokn3.so").symlink_to("libsoftokn3.so.1")
        freebl = self.provider("libfreeblpriv3.so")
        result = policy.known_dynamic_dependencies(self.root, {"usr/lib/libnss3.so": metadata(), softoken: metadata(), freebl: metadata()})
        self.assertTrue(all(entry["bundled"] for entry in result))


if __name__ == "__main__":
    unittest.main()
