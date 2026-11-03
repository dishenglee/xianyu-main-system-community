use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SQLITE_BASENAMES: &[&str] = &[
    "accounts.db",
    "live_state.db",
    "notifications.db",
    "xianyu.db",
];
const SQLITE_SUFFIXES: &[&str] = &["-wal", "-shm"];
pub const LOCAL_DATA_IMPORT_ENV: &str = "XIANYU_TAURI_ALLOW_LOCAL_DATA_IMPORT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataImportCopyReport {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub copied: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

pub fn default_data_import_candidates(current_dir: &Path) -> [PathBuf; 2] {
    [current_dir.join("data"), current_dir.join("../data")]
}

pub fn find_first_populated_data_import_dir<'a, I>(candidates: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = &'a Path>,
{
    candidates
        .into_iter()
        .find(|candidate| candidate.is_dir() && dir_contains_data_import_files(candidate))
        .map(|candidate| {
            candidate
                .canonicalize()
                .unwrap_or_else(|_| candidate.to_path_buf())
        })
}

pub fn copy_data_import_files(
    source_dir: &Path,
    destination_dir: &Path,
) -> Result<DataImportCopyReport, io::Error> {
    fs::create_dir_all(destination_dir)?;

    let mut copied = Vec::new();
    let mut skipped = Vec::new();

    for entry in fs::read_dir(source_dir)? {
        let entry = entry?;
        let src = entry.path();
        if !entry.file_type()?.is_file() {
            skipped.push(src);
            continue;
        }
        if !is_data_import_file(&src) {
            skipped.push(src);
            continue;
        }

        let file_name = entry.file_name();
        let dst = destination_dir.join(&file_name);
        fs::copy(&src, &dst)?;
        copied.push(dst);
    }

    copied.sort();
    skipped.sort();

    Ok(DataImportCopyReport {
        source: source_dir.to_path_buf(),
        destination: destination_dir.to_path_buf(),
        copied,
        skipped,
    })
}

pub fn seed_app_data_dir_from_local_sources(
    data_dir: &Path,
    cwd: Option<&Path>,
    allow_local_import: bool,
) -> Result<Option<DataImportCopyReport>, io::Error> {
    if !allow_local_import {
        return Ok(None);
    }

    let Some(current_dir) = cwd else {
        return Ok(None);
    };

    let candidates = default_data_import_candidates(current_dir);
    let source_dir = find_first_populated_data_import_dir(candidates.iter().map(PathBuf::as_path));

    match source_dir {
        Some(_) if dir_contains_data_import_files(data_dir) => Ok(None),
        Some(source_dir) => copy_data_import_files(&source_dir, data_dir).map(Some),
        None => Ok(None),
    }
}

pub fn is_data_import_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };

    SQLITE_BASENAMES.iter().any(|base| {
        file_name == *base
            || SQLITE_SUFFIXES
                .iter()
                .any(|suffix| file_name == format!("{base}{suffix}"))
    })
}

pub fn local_data_import_enabled() -> bool {
    local_data_import_enabled_for(
        env::var(LOCAL_DATA_IMPORT_ENV).ok().as_deref(),
        cfg!(debug_assertions),
    )
}

pub fn local_data_import_enabled_for(value: Option<&str>, debug_assertions: bool) -> bool {
    debug_assertions
        && value
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
}

fn dir_contains_data_import_files(dir: &Path) -> bool {
    fs::read_dir(dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .any(|entry| {
            entry.file_type().map(|ty| ty.is_file()).unwrap_or(false)
                && is_data_import_file(&entry.path())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn import_file_filter_accepts_sqlite_triplet_and_sidecars() {
        assert!(is_data_import_file(Path::new("accounts.db")));
        assert!(is_data_import_file(Path::new("accounts.db-wal")));
        assert!(is_data_import_file(Path::new("accounts.db-shm")));
        assert!(is_data_import_file(Path::new("live_state.db")));
        assert!(is_data_import_file(Path::new("notifications.db")));
        assert!(is_data_import_file(Path::new("xianyu.db-wal")));

        assert!(!is_data_import_file(Path::new("license.json")));
        assert!(!is_data_import_file(Path::new("random.db")));
        assert!(!is_data_import_file(Path::new("proxy_state.json")));
    }

    #[test]
    fn copy_data_import_files_only_copies_supported_payloads() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(&source).unwrap();

        fs::write(source.join("accounts.db"), b"accounts").unwrap();
        fs::write(source.join("accounts.db-wal"), b"wal").unwrap();
        fs::write(source.join("xianyu.db-shm"), b"shm").unwrap();
        fs::write(source.join("license.json"), b"ignore").unwrap();
        fs::write(source.join("notifications.db"), b"notify").unwrap();
        fs::create_dir_all(source.join("nested")).unwrap();

        let report = copy_data_import_files(&source, &destination).unwrap();

        assert_eq!(
            report.copied,
            vec![
                destination.join("accounts.db"),
                destination.join("accounts.db-wal"),
                destination.join("notifications.db"),
                destination.join("xianyu.db-shm"),
            ]
        );
        assert!(report.skipped.contains(&source.join("nested")));
        assert_eq!(
            fs::read(destination.join("accounts.db")).unwrap(),
            b"accounts"
        );
        assert_eq!(
            fs::read(destination.join("accounts.db-wal")).unwrap(),
            b"wal"
        );
        assert_eq!(fs::read(destination.join("xianyu.db-shm")).unwrap(), b"shm");
        assert_eq!(
            fs::read(destination.join("notifications.db")).unwrap(),
            b"notify"
        );
        assert!(!destination.join("license.json").exists());
    }

    #[test]
    fn find_first_populated_data_import_dir_skips_empty_candidates() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().join("app");
        let local_data = cwd.join("data");
        let parent_data = temp.path().join("data");
        fs::create_dir_all(&local_data).unwrap();
        fs::create_dir_all(&parent_data).unwrap();
        fs::write(parent_data.join("live_state.db"), b"live").unwrap();

        let candidates = default_data_import_candidates(&cwd);
        let candidate_refs = candidates.iter().map(PathBuf::as_path);
        let selected = find_first_populated_data_import_dir(candidate_refs).unwrap();

        assert_eq!(selected, parent_data.canonicalize().unwrap());
    }

    #[test]
    fn local_data_import_policy_requires_debug_and_explicit_truthy_env() {
        assert!(!local_data_import_enabled_for(None, true));
        assert!(!local_data_import_enabled_for(Some(""), true));
        assert!(!local_data_import_enabled_for(Some("false"), true));
        assert!(local_data_import_enabled_for(Some("1"), true));
        assert!(local_data_import_enabled_for(Some("true"), true));
        assert!(!local_data_import_enabled_for(Some("true"), false));
    }

    #[test]
    fn seed_app_data_dir_from_local_sources_skips_by_default() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().join("app");
        let local_data = cwd.join("data");
        let destination = temp.path().join("app-data");
        fs::create_dir_all(&local_data).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(local_data.join("accounts.db"), b"source").unwrap();

        let report = seed_app_data_dir_from_local_sources(&destination, Some(&cwd), false).unwrap();

        assert!(report.is_none());
        assert!(!destination.join("accounts.db").exists());
    }

    #[test]
    fn seed_app_data_dir_from_local_sources_copies_only_when_explicitly_allowed() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().join("app");
        let local_data = cwd.join("data");
        let destination = temp.path().join("app-data");
        fs::create_dir_all(&local_data).unwrap();
        fs::write(local_data.join("accounts.db"), b"source").unwrap();

        let report = seed_app_data_dir_from_local_sources(&destination, Some(&cwd), true)
            .unwrap()
            .unwrap();

        assert_eq!(report.copied, vec![destination.join("accounts.db")]);
        assert_eq!(
            fs::read(destination.join("accounts.db")).unwrap(),
            b"source"
        );
    }

    #[test]
    fn seed_app_data_dir_from_local_sources_skips_when_destination_already_seeded() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().join("app");
        let local_data = cwd.join("data");
        let destination = temp.path().join("app-data");
        fs::create_dir_all(&local_data).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(local_data.join("accounts.db"), b"source").unwrap();
        fs::write(destination.join("accounts.db"), b"existing").unwrap();

        let report = seed_app_data_dir_from_local_sources(&destination, Some(&cwd), true).unwrap();

        assert!(report.is_none());
        assert_eq!(
            fs::read(destination.join("accounts.db")).unwrap(),
            b"existing"
        );
    }

    #[test]
    fn seed_app_data_dir_from_local_sources_never_copies_license() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().join("app");
        let local_data = cwd.join("data");
        let destination = temp.path().join("app-data");
        fs::create_dir_all(&local_data).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(local_data.join("accounts.db"), b"source").unwrap();
        fs::write(local_data.join("license.json"), b"source-license").unwrap();

        let report = seed_app_data_dir_from_local_sources(&destination, Some(&cwd), true)
            .unwrap()
            .unwrap();

        assert_eq!(report.copied, vec![destination.join("accounts.db")]);
        assert!(!destination.join("license.json").exists());
    }
}
