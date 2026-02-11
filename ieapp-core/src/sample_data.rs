use crate::entry;
use crate::form;
use crate::integrity::RealIntegrityProvider;
use crate::space;
use anyhow::{anyhow, Result};
use chrono::{Duration, NaiveDate};
use opendal::Operator;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const DEFAULT_SCENARIO: &str = "renewable-ops";
pub const DEFAULT_ENTRY_COUNT: usize = 5_000;
const MIN_ENTRY_COUNT: usize = 100;
const MAX_ENTRY_COUNT: usize = 20_000;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SampleDataOptions {
    pub space_id: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default = "default_entry_count")]
    pub entry_count: usize,
    #[serde(default)]
    pub seed: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SampleDataSummary {
    pub space_id: String,
    pub scenario: String,
    pub entry_count: usize,
    pub form_count: usize,
    pub forms: Vec<String>,
}

fn default_entry_count() -> usize {
    DEFAULT_ENTRY_COUNT
}

fn normalize_entry_count(entry_count: usize, form_count: usize) -> usize {
    let mut count = entry_count.clamp(MIN_ENTRY_COUNT, MAX_ENTRY_COUNT);
    if count < form_count {
        count = form_count;
    }
    count
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

fn entry_title(form_name: &str, label: &str) -> String {
    format!("{} {}", form_name, label)
}

fn date_from_offset(base: NaiveDate, offset: i64) -> String {
    (base + Duration::days(offset))
        .format("%Y-%m-%d")
        .to_string()
}

pub async fn create_sample_space(
    op: &Operator,
    root_uri: &str,
    options: &SampleDataOptions,
) -> Result<SampleDataSummary> {
    let scenario = if options.scenario.trim().is_empty() {
        DEFAULT_SCENARIO.to_string()
    } else {
        options.scenario.trim().to_string()
    };

    if scenario != DEFAULT_SCENARIO {
        return Err(anyhow!("Unknown sample data scenario: {}", scenario));
    }

    let form_defs = renewable_ops_forms();
    let form_count = form_defs.len();
    let entry_count = normalize_entry_count(options.entry_count, form_count);

    space::create_space(op, &options.space_id, root_uri).await?;
    let ws_path = format!("spaces/{}", options.space_id);

    for form_def in &form_defs {
        form::upsert_form(op, &ws_path, form_def).await?;
    }

    let form_names: Vec<String> = form_defs
        .iter()
        .filter_map(|form_def| form_def.get("name").and_then(|name| name.as_str()))
        .map(|name| name.to_string())
        .collect();
    let forms_map: std::collections::HashMap<String, Value> = form_defs
        .iter()
        .filter_map(|form_def| {
            form_def
                .get("name")
                .and_then(|name| name.as_str())
                .map(|name| (name.to_string(), form_def.clone()))
        })
        .collect();

    let weights = [0.02, 0.08, 0.2, 0.25, 0.45];
    let counts = allocate_counts(entry_count, &weights);

    let seed = options.seed.unwrap_or_else(rand::random::<u64>);
    let mut rng = StdRng::seed_from_u64(seed);

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

    let integrity = RealIntegrityProvider::from_space(op, &options.space_id).await?;
    let empty_extra = Value::Object(Map::new());

    for site_id in site_ids.iter() {
        let capacity: f64 = rng.random_range(24.0..120.0);
        let commission_offset = rng.random_range(0..900) as i64;
        let fields = json!({
            "Region": pick(&mut rng, &regions),
            "PrimarySource": pick(&mut rng, &sources),
            "CapacityMW": (capacity * 10.0).round() / 10.0,
            "CommissionedOn": date_from_offset(base_date, commission_offset),
            "Status": pick(&mut rng, &statuses)
        });
        let title = entry_title("Site", &site_id.to_uppercase());
        let form_def = forms_map
            .get("Site")
            .ok_or_else(|| anyhow!("Missing Site form definition"))?;
        let markdown =
            entry::render_markdown_for_form(&title, "Site", &[], &fields, &empty_extra, form_def);
        entry::create_entry(
            op,
            &ws_path,
            site_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
    }

    let array_count = counts[1];
    for idx in 0..array_count {
        let site_ref = pick(&mut rng, &site_id_refs);
        let capacity_kw: f64 = rng.random_range(150.0..850.0);
        let tilt: f64 = rng.random_range(10.0..35.0);
        let install_offset = rng.random_range(0..800) as i64;
        let fields = json!({
            "SiteId": site_ref,
            "ArrayType": pick(&mut rng, &array_types),
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
            &ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
    }

    let inspection_count = counts[2];
    for idx in 0..inspection_count {
        let site_ref = pick(&mut rng, &site_id_refs);
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
            "RiskLevel": pick(&mut rng, &risk_levels),
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
            &ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
    }

    let maintenance_count = counts[3];
    for idx in 0..maintenance_count {
        let site_ref = pick(&mut rng, &site_id_refs);
        let opened_offset = rng.random_range(200..1000) as i64;
        let issue_summary = format!(
            "{} diagnostics flagged in sector {}",
            pick(&mut rng, &["Voltage", "Sensor", "Cooling", "Tracking"]),
            rng.random_range(1..12)
        );
        let resolution = format!(
            "Work order scheduled.\n\n- Parts checked\n- Estimated downtime: {} hrs",
            rng.random_range(1..8)
        );
        let fields = json!({
            "SiteId": site_ref,
            "OpenedOn": date_from_offset(base_date, opened_offset),
            "Priority": pick(&mut rng, &priorities),
            "Status": pick(&mut rng, &ticket_statuses),
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
            &ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
    }

    let report_count = counts[4];
    for idx in 0..report_count {
        let site_ref = pick(&mut rng, &site_id_refs);
        let report_offset = rng.random_range(250..1200) as i64;
        let output: f64 = rng.random_range(120.0..620.0);
        let downtime: f64 = rng.random_range(0.0..6.0);
        let fields = json!({
            "SiteId": site_ref,
            "ReportDate": date_from_offset(base_date, report_offset),
            "OutputMWh": (output * 10.0).round() / 10.0,
            "DowntimeHours": (downtime * 10.0).round() / 10.0,
            "WeatherNotes": pick(&mut rng, &weather_notes)
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
            &ws_path,
            &entry_id,
            &markdown,
            "sample-generator",
            &integrity,
        )
        .await?;
    }

    Ok(SampleDataSummary {
        space_id: options.space_id.clone(),
        scenario,
        entry_count,
        form_count,
        forms: form_names,
    })
}
