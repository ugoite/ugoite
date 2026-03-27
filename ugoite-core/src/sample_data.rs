use crate::entry;
use crate::form;
use crate::integrity::RealIntegrityProvider;
use crate::space;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use opendal::Operator;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::io::{stderr, IsTerminal, Write};
use uuid::Uuid;

pub const DEFAULT_SCENARIO: &str = "renewable-ops";
pub const DEFAULT_ENTRY_COUNT: usize = 5_000;
const MAX_ENTRY_COUNT: usize = 20_000;
const SAMPLE_JOBS_DIR: &str = "sample_jobs";
const SAMPLE_JOB_READ_RETRIES: usize = 10;
const SAMPLE_JOB_READ_RETRY_DELAY_MS: u64 = 10;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SampleJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SampleDataOptions {
    pub space_id: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default = "default_entry_count")]
    pub entry_count: usize,
    #[serde(default)]
    pub seed: Option<u64>,
    /// Optional user ID to bootstrap as the space owner (active admin member).
    /// When set, the user is added to the space settings as an active admin immediately
    /// after the space is created, making the space visible via the /spaces API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SampleDataSummary {
    pub space_id: String,
    pub scenario: String,
    pub entry_count: usize,
    pub form_count: usize,
    pub forms: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SampleDataScenario {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SampleDataJob {
    pub job_id: String,
    pub space_id: String,
    pub scenario: String,
    pub entry_count: usize,
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_user_id: Option<String>,
    pub status: SampleJobStatus,
    pub status_message: Option<String>,
    pub processed_entries: usize,
    pub total_entries: usize,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub summary: Option<SampleDataSummary>,
}

fn default_entry_count() -> usize {
    DEFAULT_ENTRY_COUNT
}

fn normalize_entry_count(entry_count: usize, form_count: usize) -> usize {
    let mut count = entry_count.clamp(1, MAX_ENTRY_COUNT);
    if count < form_count {
        count = form_count;
    }
    count
}

#[derive(Clone)]
struct ResolvedSampleDataPlan {
    scenario: String,
    form_defs: Vec<Value>,
    form_count: usize,
    entry_count: usize,
}

fn resolve_sample_data_plan(options: &SampleDataOptions) -> Result<ResolvedSampleDataPlan> {
    let scenario = if options.scenario.trim().is_empty() {
        DEFAULT_SCENARIO.to_string()
    } else {
        options.scenario.trim().to_string()
    };
    let form_defs = scenario_forms(&scenario)
        .ok_or_else(|| anyhow!("Unknown sample data scenario: {}", scenario))?;
    let form_count = form_defs.len();
    let entry_count = normalize_entry_count(options.entry_count, form_count);
    Ok(ResolvedSampleDataPlan {
        scenario,
        form_defs,
        form_count,
        entry_count,
    })
}

fn allocate_counts(entry_count: usize, weights: &[f64]) -> Vec<usize> {
    let mut counts: Vec<usize> = weights
        .iter()
        .map(|weight| ((entry_count as f64) * weight).round() as usize)
        .collect();

    for count in &mut counts {
        if *count == 0 {
            *count = 1;
        }
    }

    let mut total: isize = counts.iter().sum::<usize>() as isize;
    let target = entry_count as isize;
    let mut idx = 0usize;

    let len = counts.len();
    while total != target {
        if total < target {
            let pos = idx % len;
            counts[pos] += 1;
            total += 1;
        } else if total > target {
            let pos = idx % len;
            if counts[pos] > 1 {
                counts[pos] -= 1;
                total -= 1;
            }
        }
        idx += 1;
    }

    counts
}

fn pick<'a>(rng: &mut StdRng, options: &'a [&'a str]) -> &'a str {
    let idx = rng.random_range(0..options.len());
    options[idx]
}

pub fn list_sample_scenarios() -> Vec<SampleDataScenario> {
    vec![
        SampleDataScenario {
            id: "renewable-ops".to_string(),
            label: "Renewable operations".to_string(),
            description: "Operations data for renewable energy sites.".to_string(),
        },
        SampleDataScenario {
            id: "supply-chain".to_string(),
            label: "Supply chain operations".to_string(),
            description: "Warehouse, shipment, and supplier performance logs.".to_string(),
        },
        SampleDataScenario {
            id: "municipal-infra".to_string(),
            label: "Municipal infrastructure".to_string(),
            description: "Asset inspections and maintenance work orders.".to_string(),
        },
        SampleDataScenario {
            id: "fleet-ops".to_string(),
            label: "Fleet operations".to_string(),
            description: "Vehicle usage, service tickets, and fuel reports.".to_string(),
        },
        SampleDataScenario {
            id: "lab-qa".to_string(),
            label: "Laboratory QA".to_string(),
            description: "Batch testing, calibrations, and nonconformance tracking.".to_string(),
        },
        SampleDataScenario {
            id: "retail-ops".to_string(),
            label: "Retail operations".to_string(),
            description: "Store performance, stock alerts, and delivery logs.".to_string(),
        },
    ]
}

fn renewable_ops_forms() -> Vec<Value> {
    vec![
        json!({
            "name": "Site",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "Region": {"type": "string", "required": true},
                "PrimarySource": {"type": "string", "required": true},
                "CapacityMW": {"type": "number", "required": true},
                "CommissionedOn": {"type": "date", "required": true},
                "Status": {"type": "string", "required": true}
            }
        }),
        json!({
            "name": "Array",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "SiteId": {"type": "string", "required": true},
                "ArrayType": {"type": "string", "required": true},
                "CapacityKW": {"type": "number", "required": true},
                "TiltDegrees": {"type": "number", "required": false},
                "InstalledOn": {"type": "date", "required": true}
            }
        }),
        json!({
            "name": "Inspection",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "SiteId": {"type": "string", "required": true},
                "InspectionDate": {"type": "date", "required": true},
                "ConditionScore": {"type": "number", "required": true},
                "RiskLevel": {"type": "string", "required": true},
                "Findings": {"type": "markdown", "required": false}
            }
        }),
        json!({
            "name": "MaintenanceTicket",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "SiteId": {"type": "string", "required": true},
                "OpenedOn": {"type": "date", "required": true},
                "Priority": {"type": "string", "required": true},
                "Status": {"type": "string", "required": true},
                "IssueSummary": {"type": "string", "required": true},
                "ResolutionNotes": {"type": "markdown", "required": false}
            }
        }),
        json!({
            "name": "EnergyReport",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "SiteId": {"type": "string", "required": true},
                "ReportDate": {"type": "date", "required": true},
                "OutputMWh": {"type": "number", "required": true},
                "DowntimeHours": {"type": "number", "required": false},
                "WeatherNotes": {"type": "string", "required": false}
            }
        }),
    ]
}

fn supply_chain_forms() -> Vec<Value> {
    vec![
        json!({
            "name": "Warehouse",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "Region": {"type": "string", "required": true},
                "CapacityPallets": {"type": "number", "required": true},
                "ClimateZone": {"type": "string", "required": true},
                "OpenedOn": {"type": "date", "required": true},
                "Status": {"type": "string", "required": true}
            }
        }),
        json!({
            "name": "Shipment",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "WarehouseId": {"type": "string", "required": true},
                "Carrier": {"type": "string", "required": true},
                "Mode": {"type": "string", "required": true},
                "DispatchDate": {"type": "date", "required": true},
                "ArrivalDate": {"type": "date", "required": true},
                "OnTimeRate": {"type": "number", "required": true}
            }
        }),
        json!({
            "name": "InventoryCheck",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "WarehouseId": {"type": "string", "required": true},
                "CheckDate": {"type": "date", "required": true},
                "SKUCount": {"type": "number", "required": true},
                "AccuracyPct": {"type": "number", "required": true},
                "Notes": {"type": "markdown", "required": false}
            }
        }),
        json!({
            "name": "SupplierScore",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "SupplierId": {"type": "string", "required": true},
                "ReviewDate": {"type": "date", "required": true},
                "OnTimePct": {"type": "number", "required": true},
                "QualityScore": {"type": "number", "required": true},
                "RiskLevel": {"type": "string", "required": true}
            }
        }),
        json!({
            "name": "PurchaseOrder",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "SupplierId": {"type": "string", "required": true},
                "OrderDate": {"type": "date", "required": true},
                "TotalUnits": {"type": "number", "required": true},
                "LeadTimeDays": {"type": "number", "required": true},
                "Status": {"type": "string", "required": true}
            }
        }),
    ]
}

fn municipal_infra_forms() -> Vec<Value> {
    vec![
        json!({
            "name": "Asset",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "AssetType": {"type": "string", "required": true},
                "District": {"type": "string", "required": true},
                "InstalledOn": {"type": "date", "required": true},
                "Status": {"type": "string", "required": true},
                "ConditionScore": {"type": "number", "required": true}
            }
        }),
        json!({
            "name": "Inspection",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "AssetId": {"type": "string", "required": true},
                "InspectionDate": {"type": "date", "required": true},
                "InspectorNotes": {"type": "markdown", "required": false},
                "RiskLevel": {"type": "string", "required": true},
                "ConditionScore": {"type": "number", "required": true}
            }
        }),
        json!({
            "name": "WorkOrder",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "AssetId": {"type": "string", "required": true},
                "OpenedOn": {"type": "date", "required": true},
                "Priority": {"type": "string", "required": true},
                "Status": {"type": "string", "required": true},
                "Summary": {"type": "string", "required": true}
            }
        }),
        json!({
            "name": "ServiceReport",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "AssetId": {"type": "string", "required": true},
                "ReportDate": {"type": "date", "required": true},
                "DowntimeHours": {"type": "number", "required": false},
                "CostUSD": {"type": "number", "required": true},
                "CrewSize": {"type": "number", "required": true}
            }
        }),
    ]
}

fn fleet_ops_forms() -> Vec<Value> {
    vec![
        json!({
            "name": "Vehicle",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "VehicleType": {"type": "string", "required": true},
                "Region": {"type": "string", "required": true},
                "CommissionedOn": {"type": "date", "required": true},
                "OdometerKm": {"type": "number", "required": true},
                "Status": {"type": "string", "required": true}
            }
        }),
        json!({
            "name": "RouteLog",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "VehicleId": {"type": "string", "required": true},
                "RouteDate": {"type": "date", "required": true},
                "DistanceKm": {"type": "number", "required": true},
                "Stops": {"type": "number", "required": true},
                "OnTimeRate": {"type": "number", "required": true}
            }
        }),
        json!({
            "name": "ServiceTicket",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "VehicleId": {"type": "string", "required": true},
                "OpenedOn": {"type": "date", "required": true},
                "Priority": {"type": "string", "required": true},
                "Status": {"type": "string", "required": true},
                "IssueSummary": {"type": "string", "required": true}
            }
        }),
        json!({
            "name": "FuelReport",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "VehicleId": {"type": "string", "required": true},
                "ReportDate": {"type": "date", "required": true},
                "FuelLiters": {"type": "number", "required": true},
                "CostUSD": {"type": "number", "required": true},
                "Efficiency": {"type": "number", "required": true}
            }
        }),
    ]
}

fn lab_qa_forms() -> Vec<Value> {
    vec![
        json!({
            "name": "Batch",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "ProductLine": {"type": "string", "required": true},
                "ProducedOn": {"type": "date", "required": true},
                "BatchSize": {"type": "number", "required": true},
                "Status": {"type": "string", "required": true},
                "YieldPct": {"type": "number", "required": true}
            }
        }),
        json!({
            "name": "TestRun",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "BatchId": {"type": "string", "required": true},
                "TestDate": {"type": "date", "required": true},
                "Result": {"type": "string", "required": true},
                "DefectRate": {"type": "number", "required": true},
                "Notes": {"type": "markdown", "required": false}
            }
        }),
        json!({
            "name": "Nonconformance",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "BatchId": {"type": "string", "required": true},
                "OpenedOn": {"type": "date", "required": true},
                "Severity": {"type": "string", "required": true},
                "Disposition": {"type": "string", "required": true},
                "Summary": {"type": "string", "required": true}
            }
        }),
        json!({
            "name": "CalibrationRecord",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "Instrument": {"type": "string", "required": true},
                "CalibrationDate": {"type": "date", "required": true},
                "Status": {"type": "string", "required": true},
                "NextDue": {"type": "date", "required": true},
                "Notes": {"type": "string", "required": false}
            }
        }),
    ]
}

fn retail_ops_forms() -> Vec<Value> {
    vec![
        json!({
            "name": "Store",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "Region": {"type": "string", "required": true},
                "Format": {"type": "string", "required": true},
                "OpenedOn": {"type": "date", "required": true},
                "FloorAreaSqm": {"type": "number", "required": true},
                "Status": {"type": "string", "required": true}
            }
        }),
        json!({
            "name": "StockAlert",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "StoreId": {"type": "string", "required": true},
                "AlertDate": {"type": "date", "required": true},
                "Category": {"type": "string", "required": true},
                "Severity": {"type": "string", "required": true},
                "Notes": {"type": "markdown", "required": false}
            }
        }),
        json!({
            "name": "PriceAudit",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "StoreId": {"type": "string", "required": true},
                "AuditDate": {"type": "date", "required": true},
                "ItemsChecked": {"type": "number", "required": true},
                "MismatchRate": {"type": "number", "required": true},
                "Notes": {"type": "string", "required": false}
            }
        }),
        json!({
            "name": "DailySales",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "StoreId": {"type": "string", "required": true},
                "SalesDate": {"type": "date", "required": true},
                "Transactions": {"type": "number", "required": true},
                "RevenueUSD": {"type": "number", "required": true},
                "ReturnRate": {"type": "number", "required": true}
            }
        }),
        json!({
            "name": "VendorDelivery",
            "version": 1,
            "allow_extra_attributes": "deny",
            "fields": {
                "StoreId": {"type": "string", "required": true},
                "DeliveryDate": {"type": "date", "required": true},
                "Vendor": {"type": "string", "required": true},
                "UnitsReceived": {"type": "number", "required": true},
                "OnTime": {"type": "string", "required": true}
            }
        }),
    ]
}

fn entry_title(form_name: &str, label: &str) -> String {
    format!("{} {}", form_name, label)
}

fn date_from_offset(base: NaiveDate, offset: i64) -> String {
    (base + Duration::days(offset))
        .format("%Y-%m-%d")
        .to_string()
}

fn validate_job_id(job_id: &str) -> Result<()> {
    if job_id.is_empty() {
        return Err(anyhow!("job_id is empty"));
    }
    // Strict UUID validation as requested by reviewers
    Uuid::parse_str(job_id)
        .map_err(|e| anyhow!("Invalid job_id: {}. Must be a valid UUID. ({})", job_id, e))?;
    Ok(())
}

fn job_path(job_id: &str) -> String {
    format!("{}/{}.json", SAMPLE_JOBS_DIR, job_id)
}

async fn ensure_jobs_dir(op: &Operator) -> Result<()> {
    let root = format!("{}/", SAMPLE_JOBS_DIR);
    if !op.exists(&root).await? {
        op.create_dir(&root).await?;
    }
    Ok(())
}

async fn write_job(op: &Operator, job: &SampleDataJob) -> Result<()> {
    op.write(&job_path(&job.job_id), serde_json::to_vec_pretty(job)?)
        .await?;
    Ok(())
}

async fn read_job(op: &Operator, job_id: &str) -> Result<SampleDataJob> {
    let path = job_path(job_id);
    for attempt in 0..=SAMPLE_JOB_READ_RETRIES {
        let bytes = op.read(&path).await?.to_vec();
        match serde_json::from_slice(&bytes) {
            Ok(job) => return Ok(job),
            Err(err) if bytes.is_empty() || err.classify() == serde_json::error::Category::Eof => {
                if attempt == SAMPLE_JOB_READ_RETRIES {
                    return Err(err.into());
                }
                tokio::time::sleep(std::time::Duration::from_millis(
                    SAMPLE_JOB_READ_RETRY_DELAY_MS,
                ))
                .await;
            }
            Err(err) => return Err(err.into()),
        }
    }
    unreachable!("sample job read loop must return or error")
}

struct JobProgressWriter {
    op: Operator,
    job: SampleDataJob,
    last_flushed: usize,
}

impl JobProgressWriter {
    fn new(op: Operator, mut job: SampleDataJob) -> Self {
        if job.started_at.is_none() {
            job.started_at = Some(Utc::now());
        }
        Self {
            op,
            job,
            last_flushed: 0,
        }
    }

    async fn maybe_update(&mut self, processed: usize, message: &str) -> Result<()> {
        let threshold = 50usize;
        let message_changed = self.job.status_message.as_deref() != Some(message);
        if processed < self.job.total_entries
            && processed.saturating_sub(self.last_flushed) < threshold
            && !message_changed
        {
            return Ok(());
        }

        self.job.processed_entries = processed;
        self.job.status = SampleJobStatus::Running;
        self.job.status_message = Some(message.to_string());
        self.last_flushed = processed;
        write_job(&self.op, &self.job).await?;
        Ok(())
    }

    async fn complete(&mut self, summary: &SampleDataSummary) -> Result<()> {
        self.job.status = SampleJobStatus::Completed;
        self.job.status_message = Some("Completed".to_string());
        self.job.processed_entries = self.job.total_entries;
        self.job.completed_at = Some(Utc::now());
        self.job.summary = Some(summary.clone());
        self.job.error = None;
        write_job(&self.op, &self.job).await?;
        Ok(())
    }

    async fn fail(&mut self, error: &str) -> Result<()> {
        self.job.status = SampleJobStatus::Failed;
        self.job.status_message = Some("Failed".to_string());
        self.job.completed_at = Some(Utc::now());
        self.job.error = Some(error.to_string());
        write_job(&self.op, &self.job).await?;
        Ok(())
    }
}

struct TerminalProgressWriter {
    total_entries: usize,
    last_rendered: usize,
    last_message: Option<String>,
    is_terminal: bool,
    last_line_len: usize,
}

impl TerminalProgressWriter {
    fn new(total_entries: usize) -> Self {
        Self {
            total_entries,
            last_rendered: 0,
            last_message: None,
            is_terminal: stderr().is_terminal(),
            last_line_len: 0,
        }
    }

    fn render_threshold(&self) -> usize {
        (self.total_entries / 20).max(1)
    }

    fn render_line(&self, processed: usize, message: &str) -> String {
        let width = 20usize;
        let capped = processed.min(self.total_entries);
        let percent = if self.total_entries == 0 {
            100usize
        } else {
            ((capped * 100) / self.total_entries).min(100)
        };
        let filled = if self.total_entries == 0 {
            width
        } else {
            (((capped * width) + (self.total_entries / 2)) / self.total_entries).min(width)
        };
        let bar = format!("{}{}", "#".repeat(filled), "-".repeat(width - filled));
        format!(
            "Seed progress [{}] {:>3}% ({}/{}) {}",
            bar, percent, capped, self.total_entries, message
        )
    }

    fn should_render(&self, processed: usize, message: &str) -> bool {
        let capped = processed.min(self.total_entries);
        capped == self.total_entries
            || (capped == 0 && self.last_message.is_none())
            || self.last_message.as_deref() != Some(message)
            || capped.saturating_sub(self.last_rendered) >= self.render_threshold()
    }

    fn render(&mut self, processed: usize, message: &str) -> Result<()> {
        if !self.should_render(processed, message) {
            return Ok(());
        }

        let line = self.render_line(processed, message);
        let mut err = stderr();
        if self.is_terminal {
            let padding = " ".repeat(self.last_line_len.saturating_sub(line.len()));
            write!(err, "\r{}{}", line, padding)?;
            if processed.min(self.total_entries) == self.total_entries && message == "Completed" {
                writeln!(err)?;
            }
            err.flush()?;
            self.last_line_len = line.len();
        } else {
            writeln!(err, "{line}")?;
            err.flush()?;
        }

        self.last_rendered = processed.min(self.total_entries);
        self.last_message = Some(message.to_string());
        Ok(())
    }

    fn complete(&mut self, summary: &SampleDataSummary) -> Result<()> {
        self.render(summary.entry_count, "Completed")
    }

    fn fail(&mut self, error: &str) -> Result<()> {
        let mut err = stderr();
        if self.is_terminal && self.last_line_len > 0 {
            writeln!(err)?;
            self.last_line_len = 0;
        }
        writeln!(err, "Seed progress failed: {error}")?;
        err.flush()?;
        Ok(())
    }
}

enum ProgressReporter {
    None,
    Job(Box<JobProgressWriter>),
    Terminal(TerminalProgressWriter),
}

impl ProgressReporter {
    async fn report(&mut self, processed: usize, message: &str) -> Result<()> {
        match self {
            ProgressReporter::None => {}
            ProgressReporter::Job(writer) => writer.maybe_update(processed, message).await?,
            ProgressReporter::Terminal(writer) => writer.render(processed, message)?,
        }
        Ok(())
    }

    async fn complete(&mut self, summary: &SampleDataSummary) -> Result<()> {
        match self {
            ProgressReporter::None => {}
            ProgressReporter::Job(writer) => writer.complete(summary).await?,
            ProgressReporter::Terminal(writer) => writer.complete(summary)?,
        }
        Ok(())
    }

    async fn fail(&mut self, error: &str) -> Result<()> {
        match self {
            ProgressReporter::None => {}
            ProgressReporter::Job(writer) => writer.fail(error).await?,
            ProgressReporter::Terminal(writer) => writer.fail(error)?,
        }
        Ok(())
    }
}

struct ScenarioContext<'a> {
    op: &'a Operator,
    ws_path: &'a str,
    space_id: &'a str,
    entry_count: usize,
    rng: &'a mut StdRng,
    forms_map: &'a std::collections::HashMap<String, Value>,
    progress: &'a mut ProgressReporter,
}

fn scenario_forms(scenario: &str) -> Option<Vec<Value>> {
    match scenario {
        "renewable-ops" => Some(renewable_ops_forms()),
        "supply-chain" => Some(supply_chain_forms()),
        "municipal-infra" => Some(municipal_infra_forms()),
        "fleet-ops" => Some(fleet_ops_forms()),
        "lab-qa" => Some(lab_qa_forms()),
        "retail-ops" => Some(retail_ops_forms()),
        _ => None,
    }
}

async fn generate_renewable_ops(
    op: &Operator,
    ws_path: &str,
    space_id: &str,
    entry_count: usize,
    rng: &mut StdRng,
    forms_map: &std::collections::HashMap<String, Value>,
    progress: &mut ProgressReporter,
) -> Result<()> {
    let weights = [0.02, 0.08, 0.2, 0.25, 0.45];
    let counts = allocate_counts(entry_count, &weights);

    let base_date = NaiveDate::from_ymd_opt(2024, 1, 1)
        .ok_or_else(|| anyhow!("Failed to build base date for sample data"))?;

    let regions = [
        "North Ridge",
        "Coastal Plain",
        "River Bend",
        "High Mesa",
        "Canyon Pass",
        "Sun Valley",
    ];
    let sources = ["Solar", "Wind", "Hybrid"];
    let statuses = ["Operational", "Monitoring", "Upgrade", "Seasonal"];
    let array_types = ["Monocrystalline", "Polycrystalline", "Thin Film", "Tracker"];
    let risk_levels = ["Low", "Moderate", "Elevated", "High"];
    let priorities = ["Low", "Normal", "High", "Urgent"];
    let ticket_statuses = ["Open", "In Progress", "Resolved", "Scheduled"];
    let weather_notes = [
        "Clear skies",
        "Variable winds",
        "High heat",
        "Cool morning",
        "Overcast afternoon",
        "Dry conditions",
    ];

    let site_count = counts[0];
    let site_ids: Vec<String> = (1..=site_count)
        .map(|idx| format!("site-{:03}", idx))
        .collect();
    let site_id_refs: Vec<&str> = site_ids.iter().map(|id| id.as_str()).collect();

    let integrity = RealIntegrityProvider::from_space(op, space_id).await?;
    let empty_extra = Value::Object(Map::new());
    let mut processed = 0usize;

    for site_id in site_ids.iter() {
        let capacity: f64 = rng.random_range(24.0..120.0);
        let commission_offset = rng.random_range(0..900) as i64;
        let fields = json!({
            "Region": pick(rng, &regions),
            "PrimarySource": pick(rng, &sources),
            "CapacityMW": (capacity * 10.0).round() / 10.0,
            "CommissionedOn": date_from_offset(base_date, commission_offset),
            "Status": pick(rng, &statuses)
        });
        let title = entry_title("Site", &site_id.to_uppercase());
        let form_def = forms_map
            .get("Site")
            .ok_or_else(|| anyhow!("Missing Site form definition"))?;
        let markdown =
            entry::render_markdown_for_form(&title, "Site", &[], &fields, &empty_extra, form_def);
        entry::create_entry(
            op,
            ws_path,
            site_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Sites").await?;
    }

    let array_count = counts[1];
    for idx in 0..array_count {
        let site_ref = pick(rng, &site_id_refs);
        let capacity_kw: f64 = rng.random_range(150.0..850.0);
        let tilt: f64 = rng.random_range(10.0..35.0);
        let install_offset = rng.random_range(0..800) as i64;
        let fields = json!({
            "SiteId": site_ref,
            "ArrayType": pick(rng, &array_types),
            "CapacityKW": (capacity_kw * 10.0).round() / 10.0,
            "TiltDegrees": (tilt * 10.0).round() / 10.0,
            "InstalledOn": date_from_offset(base_date, install_offset)
        });
        let entry_id = format!("array-{:05}", idx + 1);
        let title = entry_title("Array", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("Array")
            .ok_or_else(|| anyhow!("Missing Array form definition"))?;
        let markdown =
            entry::render_markdown_for_form(&title, "Array", &[], &fields, &empty_extra, form_def);
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Arrays").await?;
    }

    let inspection_count = counts[2];
    for idx in 0..inspection_count {
        let site_ref = pick(rng, &site_id_refs);
        let inspection_offset = rng.random_range(300..1100) as i64;
        let score: f64 = rng.random_range(70.0..99.0);
        let findings = format!(
            "Inspection noted stable output with minor adjustments recommended.\n\n- Follow-up in {} days\n- Monitor inverter load",
            rng.random_range(30..120)
        );
        let fields = json!({
            "SiteId": site_ref,
            "InspectionDate": date_from_offset(base_date, inspection_offset),
            "ConditionScore": (score * 10.0).round() / 10.0,
            "RiskLevel": pick(rng, &risk_levels),
            "Findings": findings
        });
        let entry_id = format!("inspection-{:05}", idx + 1);
        let title = entry_title("Inspection", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("Inspection")
            .ok_or_else(|| anyhow!("Missing Inspection form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "Inspection",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Inspections").await?;
    }

    let maintenance_count = counts[3];
    for idx in 0..maintenance_count {
        let site_ref = pick(rng, &site_id_refs);
        let opened_offset = rng.random_range(200..1000) as i64;
        let issue_summary = format!(
            "{} diagnostics flagged in sector {}",
            pick(rng, &["Voltage", "Sensor", "Cooling", "Tracking"]),
            rng.random_range(1..12)
        );
        let resolution = format!(
            "Work order scheduled.\n\n- Parts checked\n- Estimated downtime: {} hrs",
            rng.random_range(1..8)
        );
        let fields = json!({
            "SiteId": site_ref,
            "OpenedOn": date_from_offset(base_date, opened_offset),
            "Priority": pick(rng, &priorities),
            "Status": pick(rng, &ticket_statuses),
            "IssueSummary": issue_summary,
            "ResolutionNotes": resolution
        });
        let entry_id = format!("maintenance-{:05}", idx + 1);
        let title = entry_title("Maintenance", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("MaintenanceTicket")
            .ok_or_else(|| anyhow!("Missing MaintenanceTicket form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "MaintenanceTicket",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Maintenance tickets")
            .await?;
    }

    let report_count = counts[4];
    for idx in 0..report_count {
        let site_ref = pick(rng, &site_id_refs);
        let report_offset = rng.random_range(250..1200) as i64;
        let output: f64 = rng.random_range(120.0..620.0);
        let downtime: f64 = rng.random_range(0.0..6.0);
        let fields = json!({
            "SiteId": site_ref,
            "ReportDate": date_from_offset(base_date, report_offset),
            "OutputMWh": (output * 10.0).round() / 10.0,
            "DowntimeHours": (downtime * 10.0).round() / 10.0,
            "WeatherNotes": pick(rng, &weather_notes)
        });
        let entry_id = format!("report-{:05}", idx + 1);
        let title = entry_title("EnergyReport", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("EnergyReport")
            .ok_or_else(|| anyhow!("Missing EnergyReport form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "EnergyReport",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Energy reports")
            .await?;
    }

    Ok(())
}

async fn generate_supply_chain(
    op: &Operator,
    ws_path: &str,
    space_id: &str,
    entry_count: usize,
    rng: &mut StdRng,
    forms_map: &std::collections::HashMap<String, Value>,
    progress: &mut ProgressReporter,
) -> Result<()> {
    let weights = [0.05, 0.2, 0.2, 0.25, 0.3];
    let counts = allocate_counts(entry_count, &weights);
    let base_date = NaiveDate::from_ymd_opt(2024, 2, 1)
        .ok_or_else(|| anyhow!("Failed to build base date for sample data"))?;
    let regions = ["Coastal", "Inland", "Metro", "Frontier", "Valley"];
    let climates = ["Temperate", "Dry", "Cold", "Humid"];
    let statuses = ["Active", "Seasonal", "Expansion"];
    let carriers = [
        "North Logistics",
        "Skyline Freight",
        "Harbor Line",
        "Orbit Freight",
    ];
    let modes = ["Ground", "Rail", "Coastal", "Air"];
    let risk_levels = ["Low", "Moderate", "Elevated"];
    let order_statuses = ["Open", "Confirmed", "In Transit", "Complete"];

    let warehouse_count = counts[0];
    let warehouse_ids: Vec<String> = (1..=warehouse_count)
        .map(|idx| format!("wh-{:03}", idx))
        .collect();
    let warehouse_refs: Vec<&str> = warehouse_ids.iter().map(|id| id.as_str()).collect();

    let supplier_ids: Vec<String> = (1..=counts[3])
        .map(|idx| format!("supplier-{:03}", idx))
        .collect();
    let supplier_refs: Vec<&str> = supplier_ids.iter().map(|id| id.as_str()).collect();

    let integrity = RealIntegrityProvider::from_space(op, space_id).await?;
    let empty_extra = Value::Object(Map::new());
    let mut processed = 0usize;

    for warehouse_id in warehouse_ids.iter() {
        let capacity: f64 = rng.random_range(1500.0..8000.0);
        let opened_offset = rng.random_range(0..1200) as i64;
        let fields = json!({
            "Region": pick(rng, &regions),
            "CapacityPallets": (capacity * 10.0).round() / 10.0,
            "ClimateZone": pick(rng, &climates),
            "OpenedOn": date_from_offset(base_date, opened_offset),
            "Status": pick(rng, &statuses)
        });
        let title = entry_title("Warehouse", &warehouse_id.to_uppercase());
        let form_def = forms_map
            .get("Warehouse")
            .ok_or_else(|| anyhow!("Missing Warehouse form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "Warehouse",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            warehouse_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Warehouses").await?;
    }

    for idx in 0..counts[1] {
        let warehouse_ref = pick(rng, &warehouse_refs);
        let dispatch_offset = rng.random_range(100..1200) as i64;
        let arrival_offset = dispatch_offset + rng.random_range(1..12) as i64;
        let on_time: f64 = rng.random_range(85.0..99.0);
        let fields = json!({
            "WarehouseId": warehouse_ref,
            "Carrier": pick(rng, &carriers),
            "Mode": pick(rng, &modes),
            "DispatchDate": date_from_offset(base_date, dispatch_offset),
            "ArrivalDate": date_from_offset(base_date, arrival_offset),
            "OnTimeRate": (on_time * 10.0).round() / 10.0
        });
        let entry_id = format!("shipment-{:05}", idx + 1);
        let title = entry_title("Shipment", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("Shipment")
            .ok_or_else(|| anyhow!("Missing Shipment form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "Shipment",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Shipments").await?;
    }

    for idx in 0..counts[2] {
        let warehouse_ref = pick(rng, &warehouse_refs);
        let check_offset = rng.random_range(150..1200) as i64;
        let sku_count: f64 = rng.random_range(600.0..3600.0);
        let accuracy: f64 = rng.random_range(92.0..99.5);
        let fields = json!({
            "WarehouseId": warehouse_ref,
            "CheckDate": date_from_offset(base_date, check_offset),
            "SKUCount": (sku_count * 10.0).round() / 10.0,
            "AccuracyPct": (accuracy * 10.0).round() / 10.0,
            "Notes": "Cycle count completed with standard variance.".to_string()
        });
        let entry_id = format!("inventory-{:05}", idx + 1);
        let title = entry_title("InventoryCheck", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("InventoryCheck")
            .ok_or_else(|| anyhow!("Missing InventoryCheck form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "InventoryCheck",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Inventory checks")
            .await?;
    }

    for idx in 0..counts[3] {
        let supplier_ref = pick(rng, &supplier_refs);
        let review_offset = rng.random_range(80..1200) as i64;
        let on_time: f64 = rng.random_range(80.0..99.0);
        let quality: f64 = rng.random_range(70.0..98.0);
        let fields = json!({
            "SupplierId": supplier_ref,
            "ReviewDate": date_from_offset(base_date, review_offset),
            "OnTimePct": (on_time * 10.0).round() / 10.0,
            "QualityScore": (quality * 10.0).round() / 10.0,
            "RiskLevel": pick(rng, &risk_levels)
        });
        let entry_id = format!("supplier-score-{:05}", idx + 1);
        let title = entry_title("SupplierScore", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("SupplierScore")
            .ok_or_else(|| anyhow!("Missing SupplierScore form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "SupplierScore",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Supplier scores")
            .await?;
    }

    for idx in 0..counts[4] {
        let supplier_ref = pick(rng, &supplier_refs);
        let order_offset = rng.random_range(40..1200) as i64;
        let total_units: f64 = rng.random_range(200.0..4000.0);
        let lead_time: f64 = rng.random_range(3.0..28.0);
        let fields = json!({
            "SupplierId": supplier_ref,
            "OrderDate": date_from_offset(base_date, order_offset),
            "TotalUnits": (total_units * 10.0).round() / 10.0,
            "LeadTimeDays": (lead_time * 10.0).round() / 10.0,
            "Status": pick(rng, &order_statuses)
        });
        let entry_id = format!("po-{:05}", idx + 1);
        let title = entry_title("PurchaseOrder", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("PurchaseOrder")
            .ok_or_else(|| anyhow!("Missing PurchaseOrder form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "PurchaseOrder",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Purchase orders")
            .await?;
    }

    Ok(())
}

async fn generate_municipal_infra(
    op: &Operator,
    ws_path: &str,
    space_id: &str,
    entry_count: usize,
    rng: &mut StdRng,
    forms_map: &std::collections::HashMap<String, Value>,
    progress: &mut ProgressReporter,
) -> Result<()> {
    let weights = [0.08, 0.25, 0.3, 0.37];
    let counts = allocate_counts(entry_count, &weights);
    let base_date = NaiveDate::from_ymd_opt(2024, 3, 15)
        .ok_or_else(|| anyhow!("Failed to build base date for sample data"))?;
    let asset_types = [
        "Bridge",
        "PumpStation",
        "Streetlight",
        "WaterMain",
        "Substation",
    ];
    let districts = ["North", "Central", "Harbor", "East", "South"];
    let statuses = ["Operational", "Maintenance", "Upgrade"];
    let priorities = ["Low", "Normal", "High", "Urgent"];
    let work_status = ["Open", "Scheduled", "In Progress", "Complete"];
    let risk_levels = ["Low", "Moderate", "High"];

    let asset_count = counts[0];
    let asset_ids: Vec<String> = (1..=asset_count)
        .map(|idx| format!("asset-{:03}", idx))
        .collect();
    let asset_refs: Vec<&str> = asset_ids.iter().map(|id| id.as_str()).collect();

    let integrity = RealIntegrityProvider::from_space(op, space_id).await?;
    let empty_extra = Value::Object(Map::new());
    let mut processed = 0usize;

    for asset_id in asset_ids.iter() {
        let installed_offset = rng.random_range(0..2000) as i64;
        let score: f64 = rng.random_range(60.0..98.0);
        let fields = json!({
            "AssetType": pick(rng, &asset_types),
            "District": pick(rng, &districts),
            "InstalledOn": date_from_offset(base_date, installed_offset),
            "Status": pick(rng, &statuses),
            "ConditionScore": (score * 10.0).round() / 10.0
        });
        let title = entry_title("Asset", &asset_id.to_uppercase());
        let form_def = forms_map
            .get("Asset")
            .ok_or_else(|| anyhow!("Missing Asset form definition"))?;
        let markdown =
            entry::render_markdown_for_form(&title, "Asset", &[], &fields, &empty_extra, form_def);
        entry::create_entry(
            op,
            ws_path,
            asset_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Assets").await?;
    }

    for idx in 0..counts[1] {
        let asset_ref = pick(rng, &asset_refs);
        let inspection_offset = rng.random_range(200..1400) as i64;
        let score: f64 = rng.random_range(55.0..99.0);
        let fields = json!({
            "AssetId": asset_ref,
            "InspectionDate": date_from_offset(base_date, inspection_offset),
            "InspectorNotes": "Routine inspection completed.".to_string(),
            "RiskLevel": pick(rng, &risk_levels),
            "ConditionScore": (score * 10.0).round() / 10.0
        });
        let entry_id = format!("inspection-{:05}", idx + 1);
        let title = entry_title("Inspection", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("Inspection")
            .ok_or_else(|| anyhow!("Missing Inspection form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "Inspection",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Inspections").await?;
    }

    for idx in 0..counts[2] {
        let asset_ref = pick(rng, &asset_refs);
        let opened_offset = rng.random_range(100..1400) as i64;
        let fields = json!({
            "AssetId": asset_ref,
            "OpenedOn": date_from_offset(base_date, opened_offset),
            "Priority": pick(rng, &priorities),
            "Status": pick(rng, &work_status),
            "Summary": "Preventive maintenance scheduled.".to_string()
        });
        let entry_id = format!("work-{:05}", idx + 1);
        let title = entry_title("WorkOrder", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("WorkOrder")
            .ok_or_else(|| anyhow!("Missing WorkOrder form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "WorkOrder",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Work orders").await?;
    }

    for idx in 0..counts[3] {
        let asset_ref = pick(rng, &asset_refs);
        let report_offset = rng.random_range(120..1400) as i64;
        let downtime: f64 = rng.random_range(0.0..12.0);
        let cost: f64 = rng.random_range(500.0..12000.0);
        let crew: f64 = rng.random_range(2.0..12.0);
        let fields = json!({
            "AssetId": asset_ref,
            "ReportDate": date_from_offset(base_date, report_offset),
            "DowntimeHours": (downtime * 10.0).round() / 10.0,
            "CostUSD": (cost * 10.0).round() / 10.0,
            "CrewSize": (crew * 10.0).round() / 10.0
        });
        let entry_id = format!("service-{:05}", idx + 1);
        let title = entry_title("ServiceReport", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("ServiceReport")
            .ok_or_else(|| anyhow!("Missing ServiceReport form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "ServiceReport",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Service reports")
            .await?;
    }

    Ok(())
}

async fn generate_fleet_ops(
    op: &Operator,
    ws_path: &str,
    space_id: &str,
    entry_count: usize,
    rng: &mut StdRng,
    forms_map: &std::collections::HashMap<String, Value>,
    progress: &mut ProgressReporter,
) -> Result<()> {
    let weights = [0.06, 0.28, 0.33, 0.33];
    let counts = allocate_counts(entry_count, &weights);
    let base_date = NaiveDate::from_ymd_opt(2024, 4, 10)
        .ok_or_else(|| anyhow!("Failed to build base date for sample data"))?;
    let vehicle_types = ["Van", "Truck", "EV", "Hybrid"];
    let regions = ["North", "South", "East", "West"];
    let statuses = ["Active", "Scheduled", "Depot"];
    let priorities = ["Low", "Normal", "High", "Urgent"];
    let ticket_statuses = ["Open", "In Progress", "Resolved", "Scheduled"];

    let vehicle_count = counts[0];
    let vehicle_ids: Vec<String> = (1..=vehicle_count)
        .map(|idx| format!("vehicle-{:03}", idx))
        .collect();
    let vehicle_refs: Vec<&str> = vehicle_ids.iter().map(|id| id.as_str()).collect();

    let integrity = RealIntegrityProvider::from_space(op, space_id).await?;
    let empty_extra = Value::Object(Map::new());
    let mut processed = 0usize;

    for vehicle_id in vehicle_ids.iter() {
        let commission_offset = rng.random_range(0..1500) as i64;
        let odometer: f64 = rng.random_range(5000.0..180000.0);
        let fields = json!({
            "VehicleType": pick(rng, &vehicle_types),
            "Region": pick(rng, &regions),
            "CommissionedOn": date_from_offset(base_date, commission_offset),
            "OdometerKm": (odometer * 10.0).round() / 10.0,
            "Status": pick(rng, &statuses)
        });
        let title = entry_title("Vehicle", &vehicle_id.to_uppercase());
        let form_def = forms_map
            .get("Vehicle")
            .ok_or_else(|| anyhow!("Missing Vehicle form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "Vehicle",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            vehicle_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Vehicles").await?;
    }

    for idx in 0..counts[1] {
        let vehicle_ref = pick(rng, &vehicle_refs);
        let route_offset = rng.random_range(80..1400) as i64;
        let distance: f64 = rng.random_range(40.0..650.0);
        let stops: f64 = rng.random_range(3.0..22.0);
        let on_time: f64 = rng.random_range(85.0..99.0);
        let fields = json!({
            "VehicleId": vehicle_ref,
            "RouteDate": date_from_offset(base_date, route_offset),
            "DistanceKm": (distance * 10.0).round() / 10.0,
            "Stops": (stops * 10.0).round() / 10.0,
            "OnTimeRate": (on_time * 10.0).round() / 10.0
        });
        let entry_id = format!("route-{:05}", idx + 1);
        let title = entry_title("RouteLog", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("RouteLog")
            .ok_or_else(|| anyhow!("Missing RouteLog form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "RouteLog",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Route logs").await?;
    }

    for idx in 0..counts[2] {
        let vehicle_ref = pick(rng, &vehicle_refs);
        let opened_offset = rng.random_range(60..1400) as i64;
        let issue = pick(
            rng,
            &["Brake check", "Tire rotation", "Sensor fault", "Cooling"],
        );
        let fields = json!({
            "VehicleId": vehicle_ref,
            "OpenedOn": date_from_offset(base_date, opened_offset),
            "Priority": pick(rng, &priorities),
            "Status": pick(rng, &ticket_statuses),
            "IssueSummary": issue
        });
        let entry_id = format!("service-{:05}", idx + 1);
        let title = entry_title("ServiceTicket", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("ServiceTicket")
            .ok_or_else(|| anyhow!("Missing ServiceTicket form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "ServiceTicket",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Service tickets")
            .await?;
    }

    for idx in 0..counts[3] {
        let vehicle_ref = pick(rng, &vehicle_refs);
        let report_offset = rng.random_range(40..1400) as i64;
        let fuel: f64 = rng.random_range(30.0..260.0);
        let cost: f64 = rng.random_range(60.0..520.0);
        let efficiency: f64 = rng.random_range(2.8..6.5);
        let fields = json!({
            "VehicleId": vehicle_ref,
            "ReportDate": date_from_offset(base_date, report_offset),
            "FuelLiters": (fuel * 10.0).round() / 10.0,
            "CostUSD": (cost * 10.0).round() / 10.0,
            "Efficiency": (efficiency * 10.0).round() / 10.0
        });
        let entry_id = format!("fuel-{:05}", idx + 1);
        let title = entry_title("FuelReport", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("FuelReport")
            .ok_or_else(|| anyhow!("Missing FuelReport form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "FuelReport",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Fuel reports")
            .await?;
    }

    Ok(())
}

async fn generate_lab_qa(
    op: &Operator,
    ws_path: &str,
    space_id: &str,
    entry_count: usize,
    rng: &mut StdRng,
    forms_map: &std::collections::HashMap<String, Value>,
    progress: &mut ProgressReporter,
) -> Result<()> {
    let weights = [0.1, 0.3, 0.3, 0.3];
    let counts = allocate_counts(entry_count, &weights);
    let base_date = NaiveDate::from_ymd_opt(2024, 5, 5)
        .ok_or_else(|| anyhow!("Failed to build base date for sample data"))?;
    let product_lines = ["Composite", "Ceramic", "Electronics", "Polymer"];
    let statuses = ["Released", "Hold", "Review"];
    let results = ["Pass", "Pass", "Pass", "Investigate"];
    let severities = ["Minor", "Major", "Critical"];
    let dispositions = ["Rework", "Scrap", "Use as-is", "Hold"];
    let instruments = [
        "Spectrometer",
        "Pressure Rig",
        "Thermal Chamber",
        "Microscope",
    ];
    let calibration_status = ["Valid", "Due Soon", "Overdue"];

    let batch_count = counts[0];
    let batch_ids: Vec<String> = (1..=batch_count)
        .map(|idx| format!("batch-{:03}", idx))
        .collect();
    let batch_refs: Vec<&str> = batch_ids.iter().map(|id| id.as_str()).collect();

    let integrity = RealIntegrityProvider::from_space(op, space_id).await?;
    let empty_extra = Value::Object(Map::new());
    let mut processed = 0usize;

    for batch_id in batch_ids.iter() {
        let produced_offset = rng.random_range(0..900) as i64;
        let batch_size: f64 = rng.random_range(200.0..2000.0);
        let yield_pct: f64 = rng.random_range(88.0..99.5);
        let fields = json!({
            "ProductLine": pick(rng, &product_lines),
            "ProducedOn": date_from_offset(base_date, produced_offset),
            "BatchSize": (batch_size * 10.0).round() / 10.0,
            "Status": pick(rng, &statuses),
            "YieldPct": (yield_pct * 10.0).round() / 10.0
        });
        let title = entry_title("Batch", &batch_id.to_uppercase());
        let form_def = forms_map
            .get("Batch")
            .ok_or_else(|| anyhow!("Missing Batch form definition"))?;
        let markdown =
            entry::render_markdown_for_form(&title, "Batch", &[], &fields, &empty_extra, form_def);
        entry::create_entry(
            op,
            ws_path,
            batch_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Batches").await?;
    }

    for idx in 0..counts[1] {
        let batch_ref = pick(rng, &batch_refs);
        let test_offset = rng.random_range(20..900) as i64;
        let defect: f64 = rng.random_range(0.2..6.5);
        let fields = json!({
            "BatchId": batch_ref,
            "TestDate": date_from_offset(base_date, test_offset),
            "Result": pick(rng, &results),
            "DefectRate": (defect * 10.0).round() / 10.0,
            "Notes": "QA sampling completed.".to_string()
        });
        let entry_id = format!("test-{:05}", idx + 1);
        let title = entry_title("TestRun", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("TestRun")
            .ok_or_else(|| anyhow!("Missing TestRun form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "TestRun",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Test runs").await?;
    }

    for idx in 0..counts[2] {
        let batch_ref = pick(rng, &batch_refs);
        let opened_offset = rng.random_range(20..900) as i64;
        let fields = json!({
            "BatchId": batch_ref,
            "OpenedOn": date_from_offset(base_date, opened_offset),
            "Severity": pick(rng, &severities),
            "Disposition": pick(rng, &dispositions),
            "Summary": "Variance observed in batch samples.".to_string()
        });
        let entry_id = format!("nc-{:05}", idx + 1);
        let title = entry_title("Nonconformance", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("Nonconformance")
            .ok_or_else(|| anyhow!("Missing Nonconformance form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "Nonconformance",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Nonconformance records")
            .await?;
    }

    for idx in 0..counts[3] {
        let instrument = pick(rng, &instruments);
        let calibration_offset = rng.random_range(0..900) as i64;
        let next_due = calibration_offset + rng.random_range(60..240) as i64;
        let fields = json!({
            "Instrument": instrument,
            "CalibrationDate": date_from_offset(base_date, calibration_offset),
            "Status": pick(rng, &calibration_status),
            "NextDue": date_from_offset(base_date, next_due),
            "Notes": "Calibration logged.".to_string()
        });
        let entry_id = format!("cal-{:05}", idx + 1);
        let title = entry_title("CalibrationRecord", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("CalibrationRecord")
            .ok_or_else(|| anyhow!("Missing CalibrationRecord form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "CalibrationRecord",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Calibration records")
            .await?;
    }

    Ok(())
}

async fn generate_retail_ops(
    op: &Operator,
    ws_path: &str,
    space_id: &str,
    entry_count: usize,
    rng: &mut StdRng,
    forms_map: &std::collections::HashMap<String, Value>,
    progress: &mut ProgressReporter,
) -> Result<()> {
    let weights = [0.05, 0.2, 0.2, 0.3, 0.25];
    let counts = allocate_counts(entry_count, &weights);
    let base_date = NaiveDate::from_ymd_opt(2024, 6, 1)
        .ok_or_else(|| anyhow!("Failed to build base date for sample data"))?;
    let regions = ["North", "Central", "Coastal", "Metro", "Frontier"];
    let formats = ["Compact", "Standard", "Flagship", "Outlet"];
    let statuses = ["Open", "Remodel", "Seasonal"];
    let categories = ["Grocery", "Home", "Outdoor", "Electronics", "Apparel"];
    let severities = ["Low", "Moderate", "High"];
    let vendors = ["Pioneer", "Atlas", "Summit", "Evergreen"];
    let on_time = ["Yes", "Yes", "Yes", "No"];

    let store_count = counts[0];
    let store_ids: Vec<String> = (1..=store_count)
        .map(|idx| format!("store-{:03}", idx))
        .collect();
    let store_refs: Vec<&str> = store_ids.iter().map(|id| id.as_str()).collect();

    let integrity = RealIntegrityProvider::from_space(op, space_id).await?;
    let empty_extra = Value::Object(Map::new());
    let mut processed = 0usize;

    for store_id in store_ids.iter() {
        let opened_offset = rng.random_range(0..1600) as i64;
        let area: f64 = rng.random_range(400.0..2400.0);
        let fields = json!({
            "Region": pick(rng, &regions),
            "Format": pick(rng, &formats),
            "OpenedOn": date_from_offset(base_date, opened_offset),
            "FloorAreaSqm": (area * 10.0).round() / 10.0,
            "Status": pick(rng, &statuses)
        });
        let title = entry_title("Store", &store_id.to_uppercase());
        let form_def = forms_map
            .get("Store")
            .ok_or_else(|| anyhow!("Missing Store form definition"))?;
        let markdown =
            entry::render_markdown_for_form(&title, "Store", &[], &fields, &empty_extra, form_def);
        entry::create_entry(
            op,
            ws_path,
            store_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Stores").await?;
    }

    for idx in 0..counts[1] {
        let store_ref = pick(rng, &store_refs);
        let alert_offset = rng.random_range(60..1400) as i64;
        let fields = json!({
            "StoreId": store_ref,
            "AlertDate": date_from_offset(base_date, alert_offset),
            "Category": pick(rng, &categories),
            "Severity": pick(rng, &severities),
            "Notes": "Reorder threshold reached.".to_string()
        });
        let entry_id = format!("alert-{:05}", idx + 1);
        let title = entry_title("StockAlert", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("StockAlert")
            .ok_or_else(|| anyhow!("Missing StockAlert form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "StockAlert",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Stock alerts")
            .await?;
    }

    for idx in 0..counts[2] {
        let store_ref = pick(rng, &store_refs);
        let audit_offset = rng.random_range(60..1400) as i64;
        let items: f64 = rng.random_range(120.0..1200.0);
        let mismatch: f64 = rng.random_range(0.2..6.0);
        let fields = json!({
            "StoreId": store_ref,
            "AuditDate": date_from_offset(base_date, audit_offset),
            "ItemsChecked": (items * 10.0).round() / 10.0,
            "MismatchRate": (mismatch * 10.0).round() / 10.0,
            "Notes": "Price audit completed.".to_string()
        });
        let entry_id = format!("audit-{:05}", idx + 1);
        let title = entry_title("PriceAudit", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("PriceAudit")
            .ok_or_else(|| anyhow!("Missing PriceAudit form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "PriceAudit",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Price audits")
            .await?;
    }

    for idx in 0..counts[3] {
        let store_ref = pick(rng, &store_refs);
        let sales_offset = rng.random_range(20..1400) as i64;
        let transactions: f64 = rng.random_range(120.0..1200.0);
        let revenue: f64 = rng.random_range(8000.0..85000.0);
        let return_rate: f64 = rng.random_range(0.4..4.5);
        let fields = json!({
            "StoreId": store_ref,
            "SalesDate": date_from_offset(base_date, sales_offset),
            "Transactions": (transactions * 10.0).round() / 10.0,
            "RevenueUSD": (revenue * 10.0).round() / 10.0,
            "ReturnRate": (return_rate * 10.0).round() / 10.0
        });
        let entry_id = format!("sales-{:05}", idx + 1);
        let title = entry_title("DailySales", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("DailySales")
            .ok_or_else(|| anyhow!("Missing DailySales form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "DailySales",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress.report(processed, "Generating Daily sales").await?;
    }

    for idx in 0..counts[4] {
        let store_ref = pick(rng, &store_refs);
        let delivery_offset = rng.random_range(30..1400) as i64;
        let units: f64 = rng.random_range(80.0..1800.0);
        let fields = json!({
            "StoreId": store_ref,
            "DeliveryDate": date_from_offset(base_date, delivery_offset),
            "Vendor": pick(rng, &vendors),
            "UnitsReceived": (units * 10.0).round() / 10.0,
            "OnTime": pick(rng, &on_time)
        });
        let entry_id = format!("delivery-{:05}", idx + 1);
        let title = entry_title("VendorDelivery", &format!("{:05}", idx + 1));
        let form_def = forms_map
            .get("VendorDelivery")
            .ok_or_else(|| anyhow!("Missing VendorDelivery form definition"))?;
        let markdown = entry::render_markdown_for_form(
            &title,
            "VendorDelivery",
            &[],
            &fields,
            &empty_extra,
            form_def,
        );
        entry::create_entry(
            op,
            ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
        processed += 1;
        progress
            .report(processed, "Generating Vendor deliveries")
            .await?;
    }

    Ok(())
}

async fn generate_entries_for_scenario(
    scenario: &str,
    context: &mut ScenarioContext<'_>,
) -> Result<()> {
    match scenario {
        "renewable-ops" => {
            generate_renewable_ops(
                context.op,
                context.ws_path,
                context.space_id,
                context.entry_count,
                context.rng,
                context.forms_map,
                context.progress,
            )
            .await
        }
        "supply-chain" => {
            generate_supply_chain(
                context.op,
                context.ws_path,
                context.space_id,
                context.entry_count,
                context.rng,
                context.forms_map,
                context.progress,
            )
            .await
        }
        "municipal-infra" => {
            generate_municipal_infra(
                context.op,
                context.ws_path,
                context.space_id,
                context.entry_count,
                context.rng,
                context.forms_map,
                context.progress,
            )
            .await
        }
        "fleet-ops" => {
            generate_fleet_ops(
                context.op,
                context.ws_path,
                context.space_id,
                context.entry_count,
                context.rng,
                context.forms_map,
                context.progress,
            )
            .await
        }
        "lab-qa" => {
            generate_lab_qa(
                context.op,
                context.ws_path,
                context.space_id,
                context.entry_count,
                context.rng,
                context.forms_map,
                context.progress,
            )
            .await
        }
        "retail-ops" => {
            generate_retail_ops(
                context.op,
                context.ws_path,
                context.space_id,
                context.entry_count,
                context.rng,
                context.forms_map,
                context.progress,
            )
            .await
        }
        _ => Err(anyhow!("Unknown sample data scenario: {}", scenario)),
    }
}

async fn bootstrap_sample_space_owner(
    op: &Operator,
    space_id: &str,
    owner_user_id: &str,
) -> Result<()> {
    let settings_path = format!("spaces/{space_id}/settings.json");
    let mut settings: Value = if op.exists(&settings_path).await? {
        let data = op.read(&settings_path).await?;
        serde_json::from_slice(data.to_bytes().as_ref())?
    } else {
        json!({})
    };

    let now_iso = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let member = json!({
        "user_id": owner_user_id,
        "role": "admin",
        "state": "active",
        "invited_by": owner_user_id,
        "invited_at": now_iso,
        "activated_at": "bootstrap",
        "revoked_at": null
    });

    let settings_obj = settings
        .as_object_mut()
        .ok_or_else(|| anyhow!("Invalid settings.json format"))?;

    let members = settings_obj.entry("members").or_insert_with(|| json!({}));
    members
        .as_object_mut()
        .ok_or_else(|| anyhow!("Invalid members format in settings.json"))?
        .insert(owner_user_id.to_string(), member);

    let version = settings_obj.entry("membership_version").or_insert(json!(0));
    if let Some(v) = version.as_i64() {
        *version = json!(v + 1);
    }

    let data = serde_json::to_vec(&settings)?;
    op.write(&settings_path, data).await?;
    Ok(())
}

async fn create_sample_space_with_progress(
    op: &Operator,
    root_uri: &str,
    options: &SampleDataOptions,
    plan: &ResolvedSampleDataPlan,
    progress: &mut ProgressReporter,
) -> Result<SampleDataSummary> {
    progress.report(0, "Creating space").await?;
    space::create_space(op, &options.space_id, root_uri).await?;

    if let Some(owner) = options
        .owner_user_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        bootstrap_sample_space_owner(op, &options.space_id, owner).await?;
    }

    let ws_path = format!("spaces/{}", options.space_id);

    progress.report(0, "Installing forms").await?;
    for form_def in &plan.form_defs {
        form::upsert_form(op, &ws_path, form_def).await?;
    }

    let form_names: Vec<String> = plan
        .form_defs
        .iter()
        .filter_map(|form_def| form_def.get("name").and_then(|name| name.as_str()))
        .map(|name| name.to_string())
        .collect();
    let forms_map: std::collections::HashMap<String, Value> = plan
        .form_defs
        .iter()
        .filter_map(|form_def| {
            form_def
                .get("name")
                .and_then(|name| name.as_str())
                .map(|name| (name.to_string(), form_def.clone()))
        })
        .collect();

    let seed = options.seed.unwrap_or_else(rand::random::<u64>);
    let mut rng = StdRng::seed_from_u64(seed);

    let mut context = ScenarioContext {
        op,
        ws_path: &ws_path,
        space_id: &options.space_id,
        entry_count: plan.entry_count,
        rng: &mut rng,
        forms_map: &forms_map,
        progress,
    };
    generate_entries_for_scenario(&plan.scenario, &mut context).await?;

    Ok(SampleDataSummary {
        space_id: options.space_id.clone(),
        scenario: plan.scenario.clone(),
        entry_count: plan.entry_count,
        form_count: plan.form_count,
        forms: form_names,
    })
}

pub async fn create_sample_space(
    op: &Operator,
    root_uri: &str,
    options: &SampleDataOptions,
) -> Result<SampleDataSummary> {
    let plan = resolve_sample_data_plan(options)?;
    let mut progress = ProgressReporter::None;
    create_sample_space_with_progress(op, root_uri, options, &plan, &mut progress).await
}

pub async fn create_sample_space_with_terminal_progress(
    op: &Operator,
    root_uri: &str,
    options: &SampleDataOptions,
) -> Result<SampleDataSummary> {
    let plan = resolve_sample_data_plan(options)?;
    let mut progress = ProgressReporter::Terminal(TerminalProgressWriter::new(plan.entry_count));
    match create_sample_space_with_progress(op, root_uri, options, &plan, &mut progress).await {
        Ok(summary) => {
            progress.complete(&summary).await?;
            Ok(summary)
        }
        Err(err) => {
            let _ = progress.fail(&err.to_string()).await;
            Err(err)
        }
    }
}

pub async fn create_sample_space_job(
    op: &Operator,
    root_uri: &str,
    options: &SampleDataOptions,
) -> Result<SampleDataJob> {
    let plan = resolve_sample_data_plan(options)?;
    if space::space_exists(op, &options.space_id).await? {
        return Err(anyhow!("Space already exists: {}", options.space_id));
    }

    ensure_jobs_dir(op).await?;
    let job_id = Uuid::new_v4().to_string();
    let job = SampleDataJob {
        job_id: job_id.clone(),
        space_id: options.space_id.clone(),
        scenario: plan.scenario.clone(),
        entry_count: plan.entry_count,
        seed: options.seed,
        owner_user_id: options.owner_user_id.clone(),
        status: SampleJobStatus::Queued,
        status_message: Some("Queued".to_string()),
        processed_entries: 0,
        total_entries: plan.entry_count,
        started_at: None,
        completed_at: None,
        error: None,
        summary: None,
    };

    write_job(op, &job).await?;

    let op_clone = op.clone();
    let options_clone = SampleDataOptions {
        space_id: options.space_id.clone(),
        scenario: plan.scenario.clone(),
        entry_count: plan.entry_count,
        seed: options.seed,
        owner_user_id: options.owner_user_id.clone(),
    };
    let root_uri = root_uri.to_string();
    let plan_clone = plan.clone();

    let job_for_progress = job.clone();
    tokio::spawn(async move {
        let mut progress = ProgressReporter::Job(Box::new(JobProgressWriter::new(
            op_clone.clone(),
            job_for_progress,
        )));
        let summary = create_sample_space_with_progress(
            &op_clone,
            &root_uri,
            &options_clone,
            &plan_clone,
            &mut progress,
        )
        .await;

        match summary {
            Ok(summary) => {
                let _ = progress.complete(&summary).await;
            }
            Err(err) => {
                let _ = progress.fail(&err.to_string()).await;
            }
        }
    });

    Ok(job)
}

pub async fn get_sample_space_job(op: &Operator, job_id: &str) -> Result<SampleDataJob> {
    validate_job_id(job_id)?;
    ensure_jobs_dir(op).await?;
    let path = job_path(job_id);
    if !op.exists(&path).await? {
        return Err(anyhow!("Sample data job not found: {}", job_id));
    }
    read_job(op, job_id).await
}
