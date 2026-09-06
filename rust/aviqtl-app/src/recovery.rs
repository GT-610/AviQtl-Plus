use crate::project_io::write_atomic;
#[cfg(not(test))]
use crate::settings::application_data_root;
use aviqtl_rust_core::api::{
    build_recovery_metadata, inspect_recovery_metadata, recovery_id_from_snapshot_name,
    recovery_id_is_valid, recovery_snapshot_name_is_valid,
};
use std::collections::{HashSet, hash_map::DefaultHasher};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_BACKUP_INTERVAL: Duration = Duration::from_secs(5 * 60);
const DEFAULT_MAXIMUM_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
static RECOVERY_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEntry {
    pub id: String,
    pub snapshot_path: Option<PathBuf>,
    pub original_project_url: String,
    pub display_name: String,
    pub saved_at: String,
    pub error: Option<String>,
}

impl RecoveryEntry {
    pub fn is_valid(&self) -> bool {
        self.error.is_none() && self.snapshot_path.is_some()
    }
}

pub(crate) struct RecoveryWrite {
    pub id: String,
    pub original_project_url: String,
    pub display_name: String,
    pub snapshot: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryOperation {
    Write,
    Remove,
}

pub(crate) struct RecoveryEvent {
    pub id: String,
    pub operation: RecoveryOperation,
    pub result: Result<String, String>,
}

enum RecoveryCommand {
    Write(RecoveryWrite),
    Remove(String),
    #[cfg(test)]
    Barrier(Sender<()>),
    Shutdown,
}

pub(crate) struct RecoveryStore {
    root: PathBuf,
    sender: Option<Sender<RecoveryCommand>>,
    result_receiver: Receiver<RecoveryEvent>,
    worker: Option<JoinHandle<()>>,
}

impl RecoveryStore {
    #[cfg(not(test))]
    pub(crate) fn new() -> Self {
        Self::with_root(default_recovery_root())
    }

    pub(crate) fn with_root(root: PathBuf) -> Self {
        let _ = cleanup_stale(&root, SystemTime::now(), DEFAULT_MAXIMUM_AGE);
        let (sender, receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let worker_root = root.clone();
        let worker = thread::Builder::new()
            .name("aviqtl-project-recovery".to_owned())
            .spawn(move || run_worker(worker_root, receiver, result_sender))
            .expect("project recovery worker must start");
        Self {
            root,
            sender: Some(sender),
            result_receiver,
            worker: Some(worker),
        }
    }

    pub(crate) fn request_write(&self, request: RecoveryWrite) -> Result<(), String> {
        self.sender
            .as_ref()
            .ok_or_else(|| "project recovery worker is unavailable".to_owned())?
            .send(RecoveryCommand::Write(request))
            .map_err(|_| "project recovery worker stopped".to_owned())
    }

    pub(crate) fn request_remove(&self, id: &str) -> Result<(), String> {
        if !recovery_id_is_valid(id) {
            return Err("invalid recovery identifier".to_owned());
        }
        self.sender
            .as_ref()
            .ok_or_else(|| "project recovery worker is unavailable".to_owned())?
            .send(RecoveryCommand::Remove(id.to_owned()))
            .map_err(|_| "project recovery worker stopped".to_owned())
    }

    pub(crate) fn entries(&self) -> Vec<RecoveryEntry> {
        scan_recoveries(&self.root)
    }

    pub(crate) fn poll(&self) -> Vec<RecoveryEvent> {
        self.result_receiver.try_iter().collect()
    }

    #[cfg(test)]
    pub(crate) fn flush(&self) {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .as_ref()
            .expect("project recovery worker is available")
            .send(RecoveryCommand::Barrier(sender))
            .expect("project recovery worker accepts barrier");
        receiver
            .recv()
            .expect("project recovery worker reaches barrier");
    }
}

impl Drop for RecoveryStore {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(RecoveryCommand::Shutdown);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) fn generate_recovery_id() -> String {
    let count = RECOVERY_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut first = DefaultHasher::new();
    nanos.hash(&mut first);
    count.hash(&mut first);
    std::process::id().hash(&mut first);
    let high = first.finish();
    let mut second = DefaultHasher::new();
    high.hash(&mut second);
    nanos.rotate_left(37).hash(&mut second);
    count.rotate_left(17).hash(&mut second);
    let low = second.finish();
    let value = (u128::from(high) << 64) | u128::from(low.max(1));
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (value >> 96) as u32,
        (value >> 80) as u16,
        (value >> 64) as u16,
        (value >> 48) as u16,
        value & 0x0000_ffff_ffff_ffff
    )
}

fn run_worker(
    root: PathBuf,
    receiver: Receiver<RecoveryCommand>,
    result_sender: Sender<RecoveryEvent>,
) {
    while let Ok(command) = receiver.recv() {
        let (id, operation, result) = match command {
            RecoveryCommand::Write(request) => {
                let id = request.id.clone();
                let result = write_recovery(&root, &request)
                    .map(|()| format!("Recovery snapshot updated for {}", request.display_name));
                (id, RecoveryOperation::Write, result)
            }
            RecoveryCommand::Remove(id) => {
                let result =
                    remove_recovery(&root, &id).map(|()| "Recovery snapshot discarded".to_owned());
                (id, RecoveryOperation::Remove, result)
            }
            #[cfg(test)]
            RecoveryCommand::Barrier(sender) => {
                let _ = sender.send(());
                continue;
            }
            RecoveryCommand::Shutdown => break,
        };
        let _ = result_sender.send(RecoveryEvent {
            id,
            operation,
            result: result.map_err(|error| error.to_string()),
        });
    }
}

fn write_recovery(root: &Path, request: &RecoveryWrite) -> io::Result<()> {
    if !recovery_id_is_valid(&request.id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid recovery identifier",
        ));
    }
    fs::create_dir_all(root)?;
    let generation = generate_recovery_id();
    let snapshot_file = format!("{}-{generation}.aviqtl", request.id);
    if !recovery_snapshot_name_is_valid(&request.id, &snapshot_file) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid recovery snapshot name",
        ));
    }
    let previous_snapshot = read_current_snapshot_name(root, &request.id);
    write_atomic(&root.join(&snapshot_file), &request.snapshot)?;
    let metadata = build_recovery_metadata(
        &request.id,
        &request.original_project_url,
        &request.display_name,
        &iso_timestamp(SystemTime::now()),
        &snapshot_file,
    )
    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid recovery metadata"))?;
    let metadata_bytes = serde_json::to_vec_pretty(&metadata)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if let Err(error) = write_atomic(&root.join(format!("{}.json", request.id)), &metadata_bytes) {
        let _ = fs::remove_file(root.join(&snapshot_file));
        return Err(error);
    }
    if let Some(previous) = previous_snapshot.filter(|previous| previous != &snapshot_file) {
        let _ = fs::remove_file(root.join(previous));
    }
    Ok(())
}

fn read_current_snapshot_name(root: &Path, id: &str) -> Option<String> {
    let metadata = fs::read(root.join(format!("{id}.json"))).ok()?;
    let inspection = inspect_recovery_metadata(id, &metadata)?;
    (inspection.status == "ok"
        && recovery_snapshot_name_is_valid(id, &inspection.metadata.snapshot_file))
    .then_some(inspection.metadata.snapshot_file)
}

fn remove_recovery(root: &Path, id: &str) -> io::Result<()> {
    if !recovery_id_is_valid(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid recovery identifier",
        ));
    }
    match fs::read_dir(root) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let Some(file_name) = file_name.to_str() else {
                    continue;
                };
                if recovery_snapshot_name_is_valid(id, file_name) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    let metadata = root.join(format!("{id}.json"));
    match fs::remove_file(metadata) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn scan_recoveries(root: &Path) -> Vec<RecoveryEntry> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut recoveries = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let id = path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut recovery = RecoveryEntry {
            id: id.clone(),
            snapshot_path: None,
            original_project_url: String::new(),
            display_name: id.clone(),
            saved_at: String::new(),
            error: None,
        };
        if !recovery_id_is_valid(&id) {
            recovery.error = Some("Recovery identifier is invalid".to_owned());
            recoveries.push(recovery);
            continue;
        }
        if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            recovery.error = Some("Recovery metadata must not be a symbolic link".to_owned());
            recoveries.push(recovery);
            continue;
        }
        let inspection = fs::read(&path)
            .ok()
            .and_then(|bytes| inspect_recovery_metadata(&id, &bytes));
        let Some(inspection) = inspection else {
            recovery.error = Some("Recovery metadata must be a valid JSON object".to_owned());
            recoveries.push(recovery);
            continue;
        };
        recovery.original_project_url = inspection.metadata.original_project_url;
        recovery.display_name = inspection.metadata.display_name;
        recovery.saved_at = inspection.metadata.saved_at;
        if inspection.status != "ok" {
            recovery.error = Some(format!("Recovery metadata status: {}", inspection.status));
            recoveries.push(recovery);
            continue;
        }
        let snapshot_path = root.join(&inspection.metadata.snapshot_file);
        match fs::symlink_metadata(&snapshot_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                recovery.error = Some("Recovery snapshot must not be a symbolic link".to_owned());
            }
            Ok(metadata) if metadata.is_file() => recovery.snapshot_path = Some(snapshot_path),
            Ok(_) => recovery.error = Some("Recovery snapshot is not a file".to_owned()),
            Err(error) => recovery.error = Some(format!("Recovery snapshot is missing: {error}")),
        }
        recoveries.push(recovery);
    }
    recoveries.sort_by(|left, right| right.saved_at.cmp(&left.saved_at));
    recoveries
}

fn cleanup_stale(root: &Path, now: SystemTime, maximum_age: Duration) -> io::Result<()> {
    let cutoff = now.checked_sub(maximum_age).unwrap_or(UNIX_EPOCH);
    let mut referenced_snapshots = HashSet::new();
    for recovery in scan_recoveries(root) {
        let metadata_path = root.join(format!("{}.json", recovery.id));
        let is_stale = fs::symlink_metadata(&metadata_path)
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| modified < cutoff);
        if is_stale && recovery_id_is_valid(&recovery.id) {
            remove_recovery(root, &recovery.id)?;
        } else if let Some(snapshot_path) = recovery.snapshot_path {
            referenced_snapshots.insert(snapshot_path);
        }
    }

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if recovery_id_from_snapshot_name(file_name).is_none()
            || referenced_snapshots.contains(&path)
        {
            continue;
        }
        let is_stale = fs::symlink_metadata(&path)
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| modified < cutoff);
        if is_stale {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(not(test))]
fn default_recovery_root() -> PathBuf {
    application_data_root().join("recovery")
}

fn iso_timestamp(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = i64::try_from(duration.as_secs()).unwrap_or(i64::MAX);
    let days = seconds / 86_400;
    let second_of_day = seconds % 86_400;
    let (year, month, day) = civil_date(days);
    let hour = second_of_day / 3_600;
    let minute = second_of_day % 3_600 / 60;
    let second = second_of_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        duration.subsec_millis()
    )
}

fn civil_date(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_io::ProjectSession;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aviqtl-app-recovery-{name}-{}-{}",
            std::process::id(),
            RECOVERY_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn recovery_write(id: String, scene_name: &str) -> RecoveryWrite {
        let project = ProjectSession::blank();
        let mut snapshot: serde_json::Value =
            serde_json::from_slice(&project.snapshot_bytes().expect("snapshot serializes"))
                .expect("snapshot parses");
        snapshot["scenes"][0]["name"] = serde_json::Value::String(scene_name.to_owned());
        RecoveryWrite {
            id,
            original_project_url: "/tmp/original.aviqtl".to_owned(),
            display_name: "Original".to_owned(),
            snapshot: serde_json::to_vec_pretty(&snapshot).expect("snapshot serializes"),
        }
    }

    #[test]
    fn generated_ids_and_timestamps_follow_the_rust_core_contract() {
        let id = generate_recovery_id();
        assert!(recovery_id_is_valid(&id));
        assert_eq!(
            iso_timestamp(UNIX_EPOCH + Duration::from_secs(946_684_800)),
            "2000-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn queued_writes_replace_the_previous_generation() {
        let root = test_root("replace");
        let store = RecoveryStore::with_root(root.clone());
        let id = generate_recovery_id();
        store
            .request_write(recovery_write(id.clone(), "First"))
            .expect("first recovery queues");
        store
            .request_write(recovery_write(id.clone(), "Second"))
            .expect("replacement recovery queues");
        store.flush();

        let entries = store.entries();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_valid());
        assert_eq!(entries[0].id, id);
        let recovered = ProjectSession::load_recovery(
            entries[0]
                .snapshot_path
                .as_deref()
                .expect("snapshot path exists"),
            &entries[0].original_project_url,
        )
        .expect("latest snapshot recovers");
        assert_eq!(recovered.document.scenes[0].name, "Second");
        assert_eq!(
            fs::read_dir(&root)
                .expect("recovery root exists")
                .flatten()
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "aviqtl"))
                .count(),
            1
        );

        drop(store);
        fs::remove_dir_all(root).expect("test recovery root removes");
    }

    #[test]
    fn stale_cleanup_removes_expired_recoveries_and_orphaned_snapshots() {
        let root = test_root("cleanup");
        let id = generate_recovery_id();
        write_recovery(&root, &recovery_write(id, "Root")).expect("recovery writes");
        let orphan_id = generate_recovery_id();
        let orphan_generation = generate_recovery_id();
        let orphan_path = root.join(format!("{orphan_id}-{orphan_generation}.aviqtl"));
        fs::write(&orphan_path, b"orphan").expect("orphan snapshot writes");

        cleanup_stale(&root, SystemTime::now(), DEFAULT_MAXIMUM_AGE)
            .expect("recent recoveries remain");
        assert_eq!(scan_recoveries(&root).len(), 1);
        assert!(orphan_path.exists());

        let future = SystemTime::now()
            .checked_add(DEFAULT_MAXIMUM_AGE + Duration::from_secs(1))
            .expect("test timestamp fits");
        cleanup_stale(&root, future, DEFAULT_MAXIMUM_AGE).expect("expired recoveries clean up");
        assert!(scan_recoveries(&root).is_empty());
        assert!(!orphan_path.exists());
        fs::remove_dir_all(root).expect("test recovery root removes");
    }

    #[test]
    fn invalid_identifiers_and_path_escape_metadata_are_never_recoverable() {
        let root = test_root("invalid");
        fs::create_dir_all(&root).expect("test recovery root creates");
        let outside = root.with_extension("outside.aviqtl");
        fs::write(&outside, b"outside").expect("outside fixture writes");
        fs::write(root.join("invalid.json"), b"{}").expect("invalid metadata writes");
        let id = generate_recovery_id();
        let metadata = serde_json::json!({
            "version": 1,
            "id": id,
            "originalProjectUrl": "",
            "displayName": "Escape",
            "savedAt": "2026-09-06T00:00:00.000Z",
            "snapshotFile": "../outside.aviqtl"
        });
        fs::write(
            root.join(format!("{id}.json")),
            serde_json::to_vec(&metadata).expect("metadata serializes"),
        )
        .expect("escape metadata writes");

        let entries = scan_recoveries(&root);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| !entry.is_valid()));
        assert_eq!(
            fs::read(&outside).expect("outside fixture remains"),
            b"outside"
        );
        remove_recovery(&root, &id).expect("valid id metadata removes safely");
        assert_eq!(
            fs::read(&outside).expect("outside fixture remains"),
            b"outside"
        );

        fs::remove_file(outside).expect("outside fixture removes");
        fs::remove_dir_all(root).expect("test recovery root removes");
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_metadata_and_snapshots_are_rejected() {
        use std::os::unix::fs::symlink;

        let root = test_root("symlink");
        fs::create_dir_all(&root).expect("test recovery root creates");
        let target = root.with_extension("symlink-target");
        fs::write(&target, b"{}").expect("target writes");
        let metadata_id = generate_recovery_id();
        symlink(&target, root.join(format!("{metadata_id}.json")))
            .expect("metadata symlink creates");

        let snapshot_id = generate_recovery_id();
        let snapshot_name = format!("{snapshot_id}-{}.aviqtl", generate_recovery_id());
        symlink(&target, root.join(&snapshot_name)).expect("snapshot symlink creates");
        let metadata = build_recovery_metadata(
            &snapshot_id,
            "",
            "Snapshot symlink",
            "2026-09-06T00:00:00.000Z",
            &snapshot_name,
        )
        .expect("metadata is valid");
        fs::write(
            root.join(format!("{snapshot_id}.json")),
            serde_json::to_vec(&metadata).expect("metadata serializes"),
        )
        .expect("metadata writes");

        let entries = scan_recoveries(&root);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| !entry.is_valid()));
        assert!(entries.iter().any(|entry| {
            entry
                .error
                .as_deref()
                .is_some_and(|error| error.contains("symbolic link"))
        }));
        remove_recovery(&root, &metadata_id).expect("metadata symlink removes");
        remove_recovery(&root, &snapshot_id).expect("snapshot symlink removes");
        assert_eq!(fs::read(&target).expect("symlink target remains"), b"{}");

        fs::remove_file(target).expect("symlink target removes");
        fs::remove_dir_all(root).expect("test recovery root removes");
    }
}
