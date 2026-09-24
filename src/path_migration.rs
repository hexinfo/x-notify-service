use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub moved: Vec<PathBuf>,
    pub conflicts: Vec<PathBuf>,
}

impl MigrationReport {
    fn append(&mut self, other: Self) {
        self.moved.extend(other.moved);
        self.conflicts.extend(other.conflicts);
    }
}

/// Move absent entries only; destination entries always win.
pub fn migrate_tree(old: &Path, new: &Path, excluded: &[&str]) -> io::Result<MigrationReport> {
    let mut report = MigrationReport::default();
    let old_meta = match fs::symlink_metadata(old) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(report),
        Err(error) => return Err(error),
    };
    if !old_meta.is_dir() {
        report.conflicts.push(old.to_path_buf());
        return Ok(report);
    }
    if excluded.is_empty()
        && fs::symlink_metadata(new).is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    {
        if let Some(parent) = new.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(old, new)?;
        report.moved.push(new.to_path_buf());
        return Ok(report);
    }
    match fs::symlink_metadata(new) {
        Ok(meta) if !meta.is_dir() => {
            report.conflicts.push(old.to_path_buf());
            return Ok(report);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir_all(new)?,
        Err(error) => return Err(error),
    }
    for entry in fs::read_dir(old)? {
        let entry = entry?;
        let source = entry.path();
        let destination = new.join(entry.file_name());
        if excluded.iter().any(|name| entry.file_name() == *name) {
            if fs::symlink_metadata(&source)?.is_dir() {
                report.conflicts.push(source);
            } else {
                fs::remove_file(source)?;
            }
            continue;
        }
        let source_meta = fs::symlink_metadata(&source)?;
        match fs::symlink_metadata(&destination) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::rename(&source, &destination)?;
                report.moved.push(destination);
            }
            Err(error) => return Err(error),
            Ok(destination_meta) if source_meta.is_dir() && destination_meta.is_dir() => {
                report.append(migrate_tree(&source, &destination, &[])?);
            }
            Ok(_) => report.conflicts.push(source),
        }
    }
    if fs::read_dir(old)?.next().is_none() {
        fs::remove_dir(old)?;
    }
    Ok(report)
}

pub fn migrate_private_dirs() -> io::Result<MigrationReport> {
    let data_base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    let mut roots = vec![crate::config::legacy_private_dir(data_base.clone())];
    if let Some(base) = dirs::config_local_dir().or_else(dirs::config_dir) {
        roots.push(crate::config::legacy_private_dir(base));
    }
    #[cfg(target_os = "linux")]
    if let Some(base) = dirs::state_dir() {
        roots.push(crate::config::legacy_private_dir(base));
    }
    #[cfg(target_os = "macos")]
    if let Some(base) = dirs::home_dir().map(|h| h.join("Library").join("Logs")) {
        roots.push(crate::config::legacy_private_dir(base));
    }
    if !any_existing(&roots)? {
        return Ok(MigrationReport::default());
    }
    #[cfg(target_os = "linux")]
    let new_instance_running = crate::single::current_instance_is_running();
    #[cfg(not(target_os = "linux"))]
    let new_instance_running = false;
    let locked = crate::single::legacy_is_locked();
    if locked && !new_instance_running {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "旧版实例仍在运行，请先停止服务",
        ));
    }
    if locked && new_instance_running {
        return Ok(MigrationReport {
            conflicts: roots.into_iter().filter(|path| path.exists()).collect(),
            moved: Vec::new(),
        });
    }
    let mut report = MigrationReport::default();
    #[cfg(target_os = "linux")]
    let excluded = [
        "port",
        "instance.lock",
        "sdk.js",
        "sdk.umd.js",
        "sdk-使用手册.md",
    ];
    #[cfg(not(target_os = "linux"))]
    let excluded = ["port", "instance.lock"];
    report.append(migrate_tree(
        &crate::config::legacy_private_dir(data_base.clone()),
        &crate::config::private_dir(data_base),
        &excluded,
    )?);
    if let Some(base) = dirs::config_local_dir().or_else(dirs::config_dir) {
        report.append(migrate_tree(
            &crate::config::legacy_private_dir(base.clone()),
            &crate::config::private_dir(base),
            &[],
        )?);
    }
    #[cfg(target_os = "linux")]
    if let Some(base) = dirs::state_dir() {
        report.append(migrate_tree(
            &crate::config::legacy_private_dir(base.clone()),
            &crate::config::private_dir(base),
            &[],
        )?);
    }
    #[cfg(target_os = "macos")]
    if let Some(base) = dirs::home_dir().map(|h| h.join("Library").join("Logs")) {
        report.append(migrate_tree(
            &crate::config::legacy_private_dir(base.clone()),
            &crate::config::private_dir(base),
            &[],
        )?);
    }
    Ok(report)
}

fn any_existing(paths: &[PathBuf]) -> io::Result<bool> {
    for path in paths {
        match fs::symlink_metadata(path) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_legacy_roots_need_no_lock_probe_or_migration() {
        let root = tempfile::tempdir().unwrap();
        assert!(!any_existing(&[root.path().join("data"), root.path().join("config")]).unwrap());
        fs::create_dir_all(root.path().join("config")).unwrap();
        assert!(any_existing(&[root.path().join("data"), root.path().join("config")]).unwrap());
    }

    #[test]
    fn conflict_keeps_both_configs_and_legacy_directory() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(old.join("config.toml"), "old").unwrap();
        fs::write(new.join("config.toml"), "new").unwrap();
        let report = migrate_tree(&old, &new, &[]).unwrap();
        assert_eq!(fs::read_to_string(new.join("config.toml")).unwrap(), "new");
        assert_eq!(fs::read_to_string(old.join("config.toml")).unwrap(), "old");
        assert_eq!(report.conflicts, vec![old.join("config.toml")]);
    }

    #[test]
    fn absent_destination_moves_unknown_file_and_second_call_is_empty() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("custom.txt"), "mine").unwrap();
        assert!(!migrate_tree(&old, &new, &[]).unwrap().moved.is_empty());
        assert_eq!(fs::read_to_string(new.join("custom.txt")).unwrap(), "mine");
        assert!(!old.exists());
        let again = migrate_tree(&old, &new, &[]).unwrap();
        assert!(again.moved.is_empty() && again.conflicts.is_empty());
    }

    #[test]
    fn ephemeral_instance_files_are_not_migrated() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("port"), "17320").unwrap();
        fs::write(old.join("instance.lock"), "").unwrap();
        fs::write(old.join("custom.txt"), "mine").unwrap();
        migrate_tree(&old, &new, &["port", "instance.lock"]).unwrap();
        assert!(!old.exists());
        assert!(!new.join("port").exists());
        assert!(!new.join("instance.lock").exists());
        assert_eq!(fs::read_to_string(new.join("custom.txt")).unwrap(), "mine");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_support_and_logs_migrate_to_namespaced_paths() {
        let root = tempfile::tempdir().unwrap();
        let support = root.path().join("Library/Application Support");
        let logs = root.path().join("Library/Logs");
        let old_support = crate::config::legacy_private_dir(support.clone());
        let new_support = crate::config::private_dir(support);
        let old_logs = crate::config::legacy_private_dir(logs.clone());
        let new_logs = crate::config::private_dir(logs);
        fs::create_dir_all(&old_support).unwrap();
        fs::create_dir_all(&old_logs).unwrap();
        fs::write(old_support.join("config.toml"), "port = 17321\n").unwrap();
        fs::write(old_support.join("user-data.txt"), "keep me").unwrap();
        fs::write(old_logs.join("service.log"), "old log").unwrap();

        migrate_tree(&old_support, &new_support, &[]).unwrap();
        migrate_tree(&old_logs, &new_logs, &[]).unwrap();

        assert_eq!(
            fs::read_to_string(new_support.join("config.toml")).unwrap(),
            "port = 17321\n"
        );
        assert_eq!(
            fs::read_to_string(new_support.join("user-data.txt")).unwrap(),
            "keep me"
        );
        assert_eq!(
            fs::read_to_string(new_logs.join("service.log")).unwrap(),
            "old log"
        );
        assert!(!old_support.exists());
        assert!(!old_logs.exists());
        assert!(
            migrate_tree(&old_support, &new_support, &[])
                .unwrap()
                .moved
                .is_empty()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_existing_config_wins_while_other_files_and_logs_migrate() {
        let root = tempfile::tempdir().unwrap();
        let support = root.path().join("Library/Application Support");
        let logs = root.path().join("Library/Logs");
        let old_support = crate::config::legacy_private_dir(support.clone());
        let new_support = crate::config::private_dir(support);
        let old_logs = crate::config::legacy_private_dir(logs.clone());
        let new_logs = crate::config::private_dir(logs);
        fs::create_dir_all(&old_support).unwrap();
        fs::create_dir_all(&new_support).unwrap();
        fs::create_dir_all(&old_logs).unwrap();
        fs::write(old_support.join("config.toml"), "port = 17321\n").unwrap();
        fs::write(new_support.join("config.toml"), "port = 17322\n").unwrap();
        fs::write(old_support.join("user-data.txt"), "keep me").unwrap();
        fs::write(old_logs.join("service.log"), "old log").unwrap();

        let report = migrate_tree(&old_support, &new_support, &[]).unwrap();
        migrate_tree(&old_logs, &new_logs, &[]).unwrap();

        assert_eq!(report.conflicts, vec![old_support.join("config.toml")]);
        assert_eq!(
            fs::read_to_string(old_support.join("config.toml")).unwrap(),
            "port = 17321\n"
        );
        assert_eq!(
            fs::read_to_string(new_support.join("config.toml")).unwrap(),
            "port = 17322\n"
        );
        assert_eq!(
            fs::read_to_string(new_support.join("user-data.txt")).unwrap(),
            "keep me"
        );
        assert_eq!(
            fs::read_to_string(new_logs.join("service.log")).unwrap(),
            "old log"
        );
        assert!(old_support.exists());
        assert!(!old_logs.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_moved_without_traversing_target() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        let target = root.path().join("target");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep"), "yes").unwrap();
        std::os::unix::fs::symlink(&target, old.join("link")).unwrap();
        migrate_tree(&old, &new, &[]).unwrap();
        assert!(
            fs::symlink_metadata(new.join("link"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(target.join("keep")).unwrap(), "yes");
    }
}
