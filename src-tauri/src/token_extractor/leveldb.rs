//! Shared local/sessionData scanning. OS credential access stays in the caller
//! and is needed only when an encrypted token candidate is actually present.

use super::{find_encrypted_token_payloads, find_plaintext_tokens, sorted_tokens};
use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct ScanResult {
    pub tokens: Vec<String>,
    pub files_read: usize,
    pub encrypted_candidates: usize,
}

pub(super) fn storage_roots(profile_root: &Path) -> [PathBuf; 2] {
    [profile_root.to_path_buf(), profile_root.join("sessionData")]
}

pub(super) fn scan_profile(
    profile_root: &Path,
    mut decrypt: impl FnMut(&[u8], &Path) -> Result<String>,
) -> Result<ScanResult> {
    let mut tokens = HashSet::new();
    let mut payloads = BTreeSet::new();
    let mut files_read = 0;
    let mut found_directory = false;
    let mut last_error = None;

    for storage_root in storage_roots(profile_root) {
        let directory = storage_root.join("Local Storage/leveldb");
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => {
                found_directory = true;
                entries
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                last_error =
                    Some(anyhow::Error::new(error).context("Could not read LevelDB directory"));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    last_error =
                        Some(anyhow::Error::new(error).context("Could not read LevelDB entry"));
                    continue;
                }
            };
            let path = entry.path();
            if !matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("ldb" | "log")
            ) {
                continue;
            }
            match fs::read(&path) {
                Ok(content) => {
                    files_read += 1;
                    let content = String::from_utf8_lossy(&content);
                    tokens.extend(find_plaintext_tokens(&content));
                    payloads.extend(
                        find_encrypted_token_payloads(&content)
                            .into_iter()
                            .map(|payload| (storage_root.clone(), payload)),
                    );
                }
                Err(error) => {
                    last_error =
                        Some(anyhow::Error::new(error).context("Could not read LevelDB file"));
                }
            }
        }
    }

    if files_read == 0 {
        if let Some(error) = last_error {
            return Err(error).context("No LevelDB files could be read");
        }
        anyhow::ensure!(found_directory, "LevelDB directory does not exist");
    }

    let encrypted_candidates = payloads.len();
    let mut decrypt_error = None;
    for (storage_root, payload) in payloads {
        match decrypt(&payload, &storage_root) {
            Ok(token) => {
                tokens.insert(token);
            }
            Err(error) => decrypt_error = Some(error),
        }
    }
    if tokens.is_empty() {
        if let Some(error) = decrypt_error {
            return Err(error).context("Could not decrypt any token candidates");
        }
    }

    Ok(ScanResult {
        tokens: sorted_tokens(tokens),
        files_read,
        encrypted_candidates,
    })
}

#[cfg(test)]
pub(super) mod fixtures {
    use super::*;

    pub struct TestDirectory(pub PathBuf);

    impl TestDirectory {
        pub fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("dqh-token-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        pub fn write(&self, relative: &str, content: &[u8]) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::TestDirectory;
    use super::*;
    use base64::Engine;

    const TOKEN: &str = "abcdefghijklmnopqrstuvwx.abcdef.ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl";

    #[test]
    fn scans_both_layouts_without_requiring_a_key_or_local_state() {
        let dir = TestDirectory::new();
        let mut binary = vec![0xff, 0];
        binary.extend_from_slice(TOKEN.as_bytes());
        binary.push(0xfe);
        dir.write("Local Storage/leveldb/one.ldb", &binary);
        dir.write(
            "sessionData/Local Storage/leveldb/two.log",
            TOKEN.as_bytes(),
        );
        dir.write("Local Storage/leveldb/ignored.txt", b"not scanned");
        let scan = scan_profile(&dir.0, |_, _| {
            panic!("plaintext must not access OS credentials")
        })
        .unwrap();
        assert_eq!(scan.tokens, vec![TOKEN]);
        assert_eq!(scan.files_read, 2);
    }

    #[test]
    fn retains_read_errors_when_no_candidate_file_is_readable() {
        let dir = TestDirectory::new();
        // Reading a directory as a file fails on all supported platforms.
        fs::create_dir_all(dir.0.join("Local Storage/leveldb/broken.ldb")).unwrap();
        let error = scan_profile(&dir.0, |_, _| unreachable!()).err().unwrap();
        assert!(format!("{error:#}").contains("Could not read LevelDB file"));
    }

    #[test]
    fn an_unreadable_layout_does_not_hide_a_working_session_layout() {
        let dir = TestDirectory::new();
        dir.write("Local Storage/leveldb", b"not a directory");
        dir.write(
            "sessionData/Local Storage/leveldb/two.log",
            TOKEN.as_bytes(),
        );
        assert_eq!(
            scan_profile(&dir.0, |_, _| unreachable!()).unwrap().tokens,
            vec![TOKEN]
        );
    }

    #[test]
    fn preserves_plaintext_when_encrypted_candidates_cannot_be_decrypted() {
        let dir = TestDirectory::new();
        let encoded = super::super::BASE64.encode(b"v10synthetic-payload");
        dir.write(
            "Local Storage/leveldb/one.log",
            format!("{TOKEN}\0dQw4w9WgXcQ:{encoded}").as_bytes(),
        );
        assert_eq!(
            scan_profile(&dir.0, |_, _| anyhow::bail!("locked keyring"))
                .unwrap()
                .tokens,
            vec![TOKEN]
        );
    }

    #[test]
    fn decrypts_each_distinct_payload_once_and_preserves_credential_errors() {
        let dir = TestDirectory::new();
        let encoded = super::super::BASE64.encode(b"v10synthetic-payload");
        let content = format!("dQw4w9WgXcQ:{encoded}");
        dir.write("Local Storage/leveldb/one.log", content.as_bytes());
        dir.write("Local Storage/leveldb/two.ldb", content.as_bytes());
        let mut attempts = 0;
        let error = scan_profile(&dir.0, |_, _| {
            attempts += 1;
            anyhow::bail!("locked keyring")
        })
        .err()
        .unwrap();
        assert_eq!(attempts, 1);
        assert!(format!("{error:#}").contains("locked keyring"));
    }

    #[test]
    fn distinguishes_missing_and_empty_storage_from_a_read_failure() {
        let dir = TestDirectory::new();
        assert!(scan_profile(&dir.0, |_, _| unreachable!()).is_err());
        fs::create_dir_all(dir.0.join("Local Storage/leveldb")).unwrap();
        assert!(scan_profile(&dir.0, |_, _| unreachable!())
            .unwrap()
            .tokens
            .is_empty());
    }
}
