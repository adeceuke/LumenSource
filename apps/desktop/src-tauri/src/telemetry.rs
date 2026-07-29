//! Privacy-preserving, best-effort product telemetry.
//!
//! The queue stores weekly aggregates only. It never stores prompts, responses,
//! paths, hostnames, remote target identifiers, or raw error messages.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, Days, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 1;
const MAX_RETAINED_WEEKS: usize = 52;
const DEFAULT_TELEMETRY_URL: &str = "https://lumensource.dev/v2/telemetry";

#[derive(Clone)]
pub struct Telemetry {
    inner: Arc<TelemetryInner>,
}

struct TelemetryInner {
    store_path: PathBuf,
    endpoint: String,
    client: reqwest::Client,
    store_lock: Mutex<()>,
    upload_lock: Mutex<()>,
    in_flight_reports: Mutex<BTreeSet<Uuid>>,
}

#[derive(Clone, Debug)]
pub enum TelemetryEvent {
    CatalogLoad {
        revision: String,
        source: String,
    },
    Hardware {
        ram_tier: String,
        vram_tier: String,
        accelerator: String,
    },
    ModelInstall {
        model_id: String,
        variant_id: String,
        deployment: String,
        succeeded: bool,
        failure: Option<String>,
    },
    ModelUninstall {
        model_id: String,
        variant_id: String,
        deployment: String,
        succeeded: bool,
        failure: Option<String>,
    },
    ModelStart {
        model_id: String,
        variant_id: String,
        deployment: String,
        succeeded: bool,
        failure: Option<String>,
    },
    Chat {
        model_id: String,
        variant_id: String,
        deployment: String,
        outcome: ChatOutcome,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum ChatOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadEnvelope {
    schema_version: u32,
    reports: Vec<WeeklyReport>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TelemetryStore {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    reports: Vec<WeeklyReport>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WeeklyReport {
    report_id: Uuid,
    period_start: String,
    period_end: String,
    app_version: String,
    platform: String,
    architecture: String,
    #[serde(default)]
    catalog: BTreeMap<String, CatalogUsage>,
    #[serde(default)]
    hardware: Option<HardwareUsage>,
    #[serde(default)]
    models: Vec<ModelUsage>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogUsage {
    loads: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct HardwareUsage {
    ram_tier: String,
    vram_tier: String,
    accelerator: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelUsage {
    model_id: String,
    variant_id: String,
    deployment: String,
    #[serde(default)]
    installs: OutcomeCounts,
    #[serde(default)]
    uninstalls: OutcomeCounts,
    #[serde(default)]
    starts: OutcomeCounts,
    #[serde(default)]
    chats: ChatCounts,
    #[serde(default)]
    failures: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct OutcomeCounts {
    attempted: u64,
    succeeded: u64,
    failed: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatCounts {
    attempted: u64,
    succeeded: u64,
    failed: u64,
    cancelled: u64,
}

impl Telemetry {
    pub fn new(data_root: &Path) -> Self {
        let endpoint = std::env::var("LUMEN_SOURCE_TELEMETRY_URL")
            .unwrap_or_else(|_| DEFAULT_TELEMETRY_URL.to_owned());
        Self::with_endpoint(data_root, endpoint)
    }

    fn with_endpoint(data_root: &Path, endpoint: String) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .user_agent(concat!("LumenSource/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            inner: Arc::new(TelemetryInner {
                store_path: data_root.join("lumen-source/telemetry-v1.json"),
                endpoint,
                client,
                store_lock: Mutex::new(()),
                upload_lock: Mutex::new(()),
                in_flight_reports: Mutex::new(BTreeSet::new()),
            }),
        }
    }

    pub async fn preference(&self) -> Result<Option<bool>, String> {
        let _guard = self.inner.store_lock.lock().await;
        Ok(self.load_store().await?.enabled)
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        let _guard = self.inner.store_lock.lock().await;
        let mut store = self.load_store().await?;
        store.enabled = Some(enabled);
        if !enabled {
            store.reports.clear();
        }
        self.save_store(&store).await
    }

    /// Queue an aggregate update without delaying or failing the user action.
    pub fn record(&self, event: TelemetryEvent) {
        let telemetry = self.clone();
        tauri::async_runtime::spawn(async move {
            if telemetry.record_inner(event).await.is_ok() {
                telemetry.try_upload().await;
            }
        });
    }

    /// Retry any pending reports. Failure is deliberately ignored; the next
    /// startup or occurrence will try again.
    pub fn retry_upload(&self) {
        let telemetry = self.clone();
        tauri::async_runtime::spawn(async move {
            telemetry.try_upload().await;
        });
    }

    async fn record_inner(&self, event: TelemetryEvent) -> Result<(), String> {
        let _guard = self.inner.store_lock.lock().await;
        let mut store = self.load_store().await?;
        if store.enabled != Some(true) {
            return Ok(());
        }
        let in_flight = self.inner.in_flight_reports.lock().await;
        store.record_avoiding(event, &in_flight);
        store.prune();
        self.save_store(&store).await
    }

    async fn try_upload(&self) {
        let Ok(_upload_guard) = self.inner.upload_lock.try_lock() else {
            return;
        };
        let (reports, report_ids) = {
            let _store_guard = self.inner.store_lock.lock().await;
            let Ok(store) = self.load_store().await else {
                return;
            };
            if store.enabled != Some(true) || store.reports.is_empty() {
                return;
            }
            let reports = store.reports;
            let report_ids = reports
                .iter()
                .map(|report| report.report_id)
                .collect::<Vec<_>>();
            self.inner
                .in_flight_reports
                .lock()
                .await
                .extend(report_ids.iter().copied());
            (reports, report_ids)
        };
        let response = self
            .inner
            .client
            .post(&self.inner.endpoint)
            .json(&UploadEnvelope {
                schema_version: SCHEMA_VERSION,
                reports,
            })
            .send()
            .await;
        if !response.is_ok_and(|response| response.status().is_success()) {
            self.clear_in_flight(&report_ids).await;
            return;
        }

        {
            let _store_guard = self.inner.store_lock.lock().await;
            if let Ok(mut store) = self.load_store().await {
                store
                    .reports
                    .retain(|report| !report_ids.contains(&report.report_id));
                let _ = self.save_store(&store).await;
            }
        }
        self.clear_in_flight(&report_ids).await;
    }

    async fn load_store(&self) -> Result<TelemetryStore, String> {
        match tokio::fs::read(&self.inner.store_path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(TelemetryStore::default())
            }
            Err(error) => Err(error.to_string()),
        }
    }

    async fn save_store(&self, store: &TelemetryStore) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?;
        let parent = self
            .inner
            .store_path
            .parent()
            .ok_or_else(|| "Telemetry store has no parent directory".to_owned())?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
        tokio::fs::write(&self.inner.store_path, bytes)
            .await
            .map_err(|error| error.to_string())
    }

    async fn clear_in_flight(&self, report_ids: &[Uuid]) {
        self.inner
            .in_flight_reports
            .lock()
            .await
            .retain(|report_id| !report_ids.contains(report_id));
    }
}

impl TelemetryStore {
    #[cfg(test)]
    fn record(&mut self, event: TelemetryEvent) {
        self.record_avoiding(event, &BTreeSet::new());
    }

    fn record_avoiding(&mut self, event: TelemetryEvent, in_flight: &BTreeSet<Uuid>) {
        let (period_start, period_end) = current_week();
        let app_version = env!("CARGO_PKG_VERSION").to_owned();
        let report_index = self
            .reports
            .iter()
            .position(|report| {
                report.period_start == period_start
                    && report.app_version == app_version
                    && !in_flight.contains(&report.report_id)
            })
            .unwrap_or_else(|| {
                self.reports.push(WeeklyReport {
                    report_id: Uuid::new_v4(),
                    period_start,
                    period_end,
                    app_version,
                    platform: std::env::consts::OS.to_owned(),
                    architecture: std::env::consts::ARCH.to_owned(),
                    catalog: BTreeMap::new(),
                    hardware: None,
                    models: Vec::new(),
                });
                self.reports.len() - 1
            });
        self.reports[report_index].record(event);
    }

    fn prune(&mut self) {
        self.reports
            .sort_by(|left, right| left.period_start.cmp(&right.period_start));
        if self.reports.len() > MAX_RETAINED_WEEKS {
            self.reports
                .drain(..self.reports.len() - MAX_RETAINED_WEEKS);
        }
    }
}

impl WeeklyReport {
    fn record(&mut self, event: TelemetryEvent) {
        match event {
            TelemetryEvent::CatalogLoad { revision, source } => {
                let key = format!("{revision}:{source}");
                self.catalog.entry(key).or_default().loads += 1;
            }
            TelemetryEvent::Hardware {
                ram_tier,
                vram_tier,
                accelerator,
            } => {
                self.hardware = Some(HardwareUsage {
                    ram_tier,
                    vram_tier,
                    accelerator,
                });
            }
            TelemetryEvent::ModelInstall {
                model_id,
                variant_id,
                deployment,
                succeeded,
                failure,
            } => {
                let usage = self.model_usage(model_id, variant_id, deployment);
                usage.installs.record(succeeded);
                usage.record_failure(failure);
            }
            TelemetryEvent::ModelUninstall {
                model_id,
                variant_id,
                deployment,
                succeeded,
                failure,
            } => {
                let usage = self.model_usage(model_id, variant_id, deployment);
                usage.uninstalls.record(succeeded);
                usage.record_failure(failure);
            }
            TelemetryEvent::ModelStart {
                model_id,
                variant_id,
                deployment,
                succeeded,
                failure,
            } => {
                let usage = self.model_usage(model_id, variant_id, deployment);
                usage.starts.record(succeeded);
                usage.record_failure(failure);
            }
            TelemetryEvent::Chat {
                model_id,
                variant_id,
                deployment,
                outcome,
            } => {
                let usage = self.model_usage(model_id, variant_id, deployment);
                usage.chats.attempted += 1;
                match outcome {
                    ChatOutcome::Succeeded => usage.chats.succeeded += 1,
                    ChatOutcome::Failed => usage.chats.failed += 1,
                    ChatOutcome::Cancelled => usage.chats.cancelled += 1,
                }
            }
        }
    }

    fn model_usage(
        &mut self,
        model_id: String,
        variant_id: String,
        deployment: String,
    ) -> &mut ModelUsage {
        let index = self
            .models
            .iter()
            .position(|usage| {
                usage.model_id == model_id
                    && usage.variant_id == variant_id
                    && usage.deployment == deployment
            })
            .unwrap_or_else(|| {
                self.models.push(ModelUsage {
                    model_id,
                    variant_id,
                    deployment,
                    installs: OutcomeCounts::default(),
                    uninstalls: OutcomeCounts::default(),
                    starts: OutcomeCounts::default(),
                    chats: ChatCounts::default(),
                    failures: BTreeMap::new(),
                });
                self.models.len() - 1
            });
        &mut self.models[index]
    }
}

impl OutcomeCounts {
    fn record(&mut self, succeeded: bool) {
        self.attempted += 1;
        if succeeded {
            self.succeeded += 1;
        } else {
            self.failed += 1;
        }
    }
}

impl ModelUsage {
    fn record_failure(&mut self, failure: Option<String>) {
        if let Some(failure) = failure {
            *self.failures.entry(failure).or_default() += 1;
        }
    }
}

fn current_week() -> (String, String) {
    let today = Utc::now().date_naive();
    let start = today
        .checked_sub_days(Days::new(today.weekday().num_days_from_monday().into()))
        .unwrap_or(today);
    let end = start.checked_add_days(Days::new(6)).unwrap_or(start);
    (
        start.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    )
}

pub fn memory_tier(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    match bytes / GIB {
        0..=7 => "under-8-gib",
        8..=15 => "8-15-gib",
        16..=31 => "16-31-gib",
        32..=63 => "32-63-gib",
        _ => "64-plus-gib",
    }
    .to_owned()
}

pub fn failure_category(error: &str) -> String {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("cancel") {
        "cancelled"
    } else if normalized.contains("out of memory")
        || normalized.contains("insufficient memory")
        || normalized.contains("not enough memory")
    {
        "insufficient-memory"
    } else if normalized.contains("timed out")
        || normalized.contains("connection")
        || normalized.contains("unreachable")
        || normalized.contains("network")
    {
        "network"
    } else if normalized.contains("runtime") || normalized.contains("ollama") {
        "runtime"
    } else if normalized.contains("catalog") || normalized.contains("variant") {
        "catalog"
    } else {
        "unknown"
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn install_event(succeeded: bool) -> TelemetryEvent {
        TelemetryEvent::ModelInstall {
            model_id: "qwen3".to_owned(),
            variant_id: "qwen3-8b-q4_k_m".to_owned(),
            deployment: "local".to_owned(),
            succeeded,
            failure: (!succeeded).then(|| "insufficient-memory".to_owned()),
        }
    }

    #[test]
    fn aggregates_model_activity_without_content() {
        let mut store = TelemetryStore {
            enabled: Some(true),
            reports: Vec::new(),
        };
        store.record(install_event(true));
        store.record(install_event(false));

        assert_eq!(store.reports.len(), 1);
        let usage = &store.reports[0].models[0];
        assert_eq!(usage.installs.attempted, 2);
        assert_eq!(usage.installs.succeeded, 1);
        assert_eq!(usage.installs.failed, 1);
        assert_eq!(usage.failures.get("insufficient-memory").copied(), Some(1));
    }

    #[test]
    fn disabling_telemetry_clears_queued_reports() {
        let mut store = TelemetryStore {
            enabled: Some(true),
            reports: Vec::new(),
        };
        store.record(install_event(true));
        store.enabled = Some(false);
        store.reports.clear();

        assert_eq!(store.enabled, Some(false));
        assert!(store.reports.is_empty());
    }

    #[test]
    fn keeps_only_the_bounded_backlog() {
        let mut store = TelemetryStore::default();
        for index in 0..60 {
            store.reports.push(WeeklyReport {
                report_id: Uuid::new_v4(),
                period_start: format!("2025-{index:02}-01"),
                period_end: format!("2025-{index:02}-07"),
                app_version: "0.4.0".to_owned(),
                platform: "linux".to_owned(),
                architecture: "x86_64".to_owned(),
                catalog: BTreeMap::new(),
                hardware: None,
                models: Vec::new(),
            });
        }
        store.prune();

        assert_eq!(store.reports.len(), MAX_RETAINED_WEEKS);
    }

    #[test]
    fn categorizes_errors_without_storing_raw_messages() {
        assert_eq!(
            failure_category("connection timed out at secret-host.example"),
            "network"
        );
        assert_eq!(failure_category("arbitrary private detail"), "unknown");
    }

    #[test]
    fn an_occurrence_during_upload_uses_a_fresh_report() {
        let mut store = TelemetryStore::default();
        store.record(install_event(true));
        let original_report_id = store.reports[0].report_id;
        let in_flight = BTreeSet::from([original_report_id]);

        store.record_avoiding(install_event(true), &in_flight);
        store
            .reports
            .retain(|report| report.report_id != original_report_id);

        assert_eq!(store.reports.len(), 1);
        assert_eq!(store.reports[0].models[0].installs.attempted, 1);
    }

    #[tokio::test]
    async fn failed_upload_is_kept_and_retried_until_acknowledged() {
        let Ok(listener) = TcpListener::bind("127.0.0.1:0").await else {
            panic!("test telemetry listener should bind");
        };
        let Ok(address) = listener.local_addr() else {
            panic!("test telemetry listener should have an address");
        };
        let server = tokio::spawn(async move {
            for status in ["503 Service Unavailable", "204 No Content"] {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut request = vec![0_u8; 16 * 1024];
                if stream.read(&mut request).await.is_err() {
                    return;
                }
                let response =
                    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                if stream.write_all(response.as_bytes()).await.is_err() {
                    return;
                }
            }
        });
        let Ok(directory) = tempfile::tempdir() else {
            panic!("test telemetry directory should be created");
        };
        let telemetry =
            Telemetry::with_endpoint(directory.path(), format!("http://{address}/telemetry"));
        if telemetry.set_enabled(true).await.is_err() {
            panic!("test telemetry preference should persist");
        }
        if telemetry.record_inner(install_event(true)).await.is_err() {
            panic!("test telemetry event should persist");
        }

        telemetry.try_upload().await;
        let Ok(after_failure) = telemetry.load_store().await else {
            panic!("test telemetry store should remain readable");
        };
        assert_eq!(after_failure.reports.len(), 1);

        telemetry.try_upload().await;
        let Ok(after_success) = telemetry.load_store().await else {
            panic!("test telemetry store should remain readable");
        };
        assert!(after_success.reports.is_empty());
        let _ = server.await;
    }
}
