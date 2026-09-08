//! Helper-owned, bounded settings and diagnostics; raw commands never reach logs.
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use download_manager_engine::persistence::TaskMetadata;
use download_manager_engine::progress::ProgressPolicy;
use download_manager_engine::scheduler::{
    ConcurrencyLimits, DownloadScheduler, SchedulerOptions, WorkerCount,
};
use download_manager_engine::task::{RetryPolicy, TaskEngineOptions};
use download_manager_protocol::{SettingsDescription, SettingsPatchInput};
use serde::{Deserialize, Serialize};

use crate::HostError;
const SETTINGS_LIMIT: u64 = 64 * 1024;
const LOG_LIMIT: u64 = 64 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    settings: SettingsDescription,
}

pub(crate) struct SettingsStore {
    root: PathBuf,
    pub current: SettingsDescription,
}
impl SettingsStore {
    pub fn load(root: &Path, destination: Option<&Path>) -> Result<Self, HostError> {
        let file = root.join("settings.json");
        let current = if file.try_exists().map_err(|_| HostError::Configuration)? {
            let mut document: Document =
                serde_json::from_slice(&read_bounded(&file, SETTINGS_LIMIT)?)
                    .map_err(|_| HostError::Configuration)?;
            // Version 1 did not have verbose diagnostics; Deserialize defaults it off.
            if !matches!(document.version, 1 | 2) {
                return Err(HostError::Configuration);
            }
            document.version = 2;
            validate(&document.settings)?;
            document.settings
        } else {
            SettingsDescription {
                destination: destination
                    .and_then(Path::to_str)
                    .ok_or(HostError::Configuration)?
                    .to_owned(),
                default_workers: 4,
                global_concurrency: 16,
                per_host_concurrency: 8,
                retry_limit: 5,
                keep_partial_on_cancel: true,
                keep_partial_on_failure: true,
                verbose_logging: false,
            }
        };
        Ok(Self {
            root: root.to_owned(),
            current,
        })
    }
    pub fn patched(&self, patch: &SettingsPatchInput) -> Result<SettingsDescription, HostError> {
        let mut value = self.current.clone();
        if let Some(destination) = &patch.destination {
            value.destination.clone_from(destination);
        }
        macro_rules! apply { ($($field:ident),+) => { $(if let Some(field) = patch.$field { value.$field = field; })+ }; }
        apply!(
            default_workers,
            global_concurrency,
            per_host_concurrency,
            retry_limit,
            keep_partial_on_cancel,
            keep_partial_on_failure,
            verbose_logging
        );
        validate(&value)?;
        Ok(value)
    }
    pub fn save(&mut self, value: SettingsDescription) -> Result<(), HostError> {
        validate(&value)?;
        let bytes = serde_json::to_vec(&Document {
            version: 2,
            settings: value.clone(),
        })
        .map_err(|_| HostError::Configuration)?;
        atomic_write(&self.root.join("settings.json"), &bytes)?;
        self.current = value;
        self.log(Diagnostic::SettingsApplied);
        Ok(())
    }
    pub fn log(&self, event: Diagnostic) {
        if matches!(event, Diagnostic::CommandAccepted) && !self.current.verbose_logging {
            return;
        }
        // Diagnostics are best effort and cannot change task correctness.
        let _ = self.write_log(event);
    }
    fn write_log(&self, event: Diagnostic) -> Result<(), HostError> {
        let path = self.root.join("diagnostics.log");
        let mut bytes = if path.try_exists().map_err(|_| HostError::Configuration)? {
            read_bounded(&path, LOG_LIMIT)?
        } else {
            Vec::new()
        };
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| HostError::Configuration)?
            .as_secs();
        let line = format!("{timestamp} {}\n", event.label());
        if bytes.len() + line.len()
            > usize::try_from(LOG_LIMIT).map_err(|_| HostError::Configuration)?
        {
            atomic_write(&self.root.join("diagnostics.previous.log"), &bytes)?;
            bytes.clear();
        }
        bytes.extend_from_slice(line.as_bytes());
        atomic_write(&path, &bytes)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Diagnostic {
    Started,
    SettingsApplied,
    CommandAccepted,
    Failed,
}
impl Diagnostic {
    const fn label(self) -> &'static str {
        match self {
            Self::Started => "host_started",
            Self::SettingsApplied => "settings_applied",
            Self::CommandAccepted => "command_received",
            Self::Failed => "task_failed",
        }
    }
}

pub(crate) fn engine_configuration(
    value: &SettingsDescription,
) -> Result<(TaskEngineOptions, DownloadScheduler), HostError> {
    let workers =
        WorkerCount::try_from(value.default_workers).map_err(|_| HostError::Configuration)?;
    let retry = RetryPolicy::new(
        value.retry_limit,
        Duration::from_millis(250),
        Duration::from_secs(30),
        Duration::from_secs(300),
    )
    .map_err(|_| HostError::Configuration)?;
    let options = TaskEngineOptions::new(workers, retry, ProgressPolicy::default(), 4096)
        .map_err(|_| HostError::Configuration)?
        .with_failure_retention(value.keep_partial_on_failure);
    let limits = ConcurrencyLimits::new(
        usize::from(value.per_host_concurrency),
        usize::from(value.global_concurrency),
    )
    .map_err(|_| HostError::Configuration)?;
    let defaults = SchedulerOptions::default();
    let scheduler = DownloadScheduler::with_options(
        SchedulerOptions::new(
            limits,
            defaults.tail_hedge_delay(),
            defaults.unknown_stream_limit(),
        )
        .map_err(|_| HostError::Configuration)?,
    )
    .map_err(|_| HostError::Configuration)?;
    Ok((options, scheduler))
}
fn validate(value: &SettingsDescription) -> Result<(), HostError> {
    TaskMetadata::new(
        "https://settings.invalid/",
        Path::new(&value.destination),
        "settings-validation",
    )
    .map_err(|_| HostError::Configuration)?;
    let _ = engine_configuration(value)?;
    Ok(())
}
fn ordinary_file(path: &Path) -> Result<(), HostError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Configuration)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(HostError::Configuration);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(HostError::Configuration);
        }
    }
    Ok(())
}
fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, HostError> {
    ordinary_file(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| HostError::Configuration)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Configuration)?;
    if bytes.len() as u64 > maximum {
        return Err(HostError::Configuration);
    }
    Ok(bytes)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), HostError> {
    if path.try_exists().map_err(|_| HostError::Configuration)? {
        ordinary_file(path)?;
    }
    // Only this host owns the state-store lock. create_new refuses stale/hostile temp entries.
    let temporary = path.with_extension("write-new");
    if temporary
        .try_exists()
        .map_err(|_| HostError::Configuration)?
    {
        ordinary_file(&temporary)?;
        fs::remove_file(&temporary).map_err(|_| HostError::Configuration)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| HostError::Configuration)?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| HostError::Configuration)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_settings_round_trip_migration_and_bounded_private_logs() {
        let root = std::env::temp_dir().join(format!(
            "dm-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("root");
        let mut store = SettingsStore::load(&root, Some(&root)).expect("defaults");
        assert!(!store.current.verbose_logging);
        store.current.verbose_logging = true;
        store.save(store.current.clone()).expect("save");
        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("settings.json")).expect("read"))
                .expect("json");
        json["version"] = serde_json::json!(1);
        json["settings"]
            .as_object_mut()
            .expect("settings")
            .remove("verbose_logging");
        fs::write(
            root.join("settings.json"),
            serde_json::to_vec(&json).expect("encode"),
        )
        .expect("legacy");
        let loaded = SettingsStore::load(&root, None).expect("migrate");
        assert!(!loaded.current.verbose_logging);
        json["version"] = serde_json::json!(99);
        fs::write(
            root.join("settings.json"),
            serde_json::to_vec(&json).expect("encode"),
        )
        .expect("future");
        assert!(SettingsStore::load(&root, None).is_err());
        fs::write(
            root.join("diagnostics.log"),
            vec![b'x'; usize::try_from(LOG_LIMIT).expect("log size")],
        )
        .expect("fill log");
        store.log(Diagnostic::CommandAccepted);
        assert!(root.join("diagnostics.previous.log").exists());
        let bytes = fs::read(root.join("diagnostics.log")).expect("log");
        assert!(bytes.len() as u64 <= LOG_LIMIT);
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(!text.contains("https:"));
        assert!(!text.contains(&root.to_string_lossy().to_string()));
        let mut invalid = store.current.clone();
        invalid.retry_limit = 21;
        assert!(store.save(invalid).is_err());
        let mut invalid = store.current.clone();
        invalid.destination = "../escape".to_owned();
        assert!(store.save(invalid).is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
