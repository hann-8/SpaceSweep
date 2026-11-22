use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use uuid::Uuid;

use crate::discovery::ReviewLevel;
use crate::store::Store;

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    choices: Vec<Choice>,
}

#[derive(Deserialize, Serialize)]
struct Choice {
    level: ReviewLevel,
    label: String,
    group_id: Option<String>,
    scan_ids: Vec<String>,
    findings: Vec<Value>,
}

#[derive(Deserialize)]
struct Submission {
    choices: BTreeSet<usize>,
}

#[derive(Serialize)]
struct Selection<'a> {
    schema_version: u32,
    scan_ids: BTreeSet<&'a str>,
    include_groups: BTreeSet<&'a str>,
    choices: Vec<&'a Choice>,
}

impl Manifest {
    fn select(&self, submission: Submission) -> Result<Selection<'_>> {
        ensure!(
            !submission.choices.is_empty(),
            "select at least one review item"
        );
        let choices: Vec<_> = submission
            .choices
            .into_iter()
            .map(|id| self.choices.get(id).context("unknown review choice"))
            .collect::<Result<_>>()?;
        let mut scan_ids = BTreeSet::new();
        let mut include_groups = BTreeSet::new();
        for choice in &choices {
            if choice.level == ReviewLevel::NeedsReview
                && !choice.scan_ids.is_empty()
                && let Some(group) = choice.group_id.as_deref()
            {
                scan_ids.extend(choice.scan_ids.iter().map(String::as_str));
                include_groups.insert(group);
            }
        }
        Ok(Selection {
            schema_version: 1,
            scan_ids,
            include_groups,
            choices,
        })
    }
}

fn status(value: Value) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{value}")?;
    stdout.flush()
}

fn respond(request: Request, code: u16, content_type: &str, body: &str) -> io::Result<()> {
    let header =
        |name: &str, value: &str| Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap();
    request.respond(
        Response::from_string(body)
            .with_status_code(StatusCode(code))
            .with_header(header("Content-Type", content_type))
            .with_header(header("Cache-Control", "no-store")),
    )
}

fn read_submission(request: &mut Request) -> Result<Submission> {
    const MAX_BYTES: usize = 1024 * 1024;
    let mut body = Vec::new();
    request
        .as_reader()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut body)?;
    ensure!(body.len() <= MAX_BYTES, "review submission is too large");
    Ok(serde_json::from_slice(&body)?)
}

pub fn serve(
    store: &Store,
    report: &Path,
    manifest: &Path,
    timeout_seconds: u64,
    cancel: &AtomicBool,
) -> Result<()> {
    let manifest: Manifest = serde_json::from_slice(&fs::read(manifest)?)?;
    ensure!(manifest.schema_version == 1, "unsupported review manifest");
    if manifest.choices.is_empty() {
        status(json!({"status": "no_choices"}))?;
        return Ok(());
    }
    let html = fs::read_to_string(report)?;
    let server = Server::http(("127.0.0.1", 0))
        .map_err(|error| anyhow!("open local review page: {error}"))?;
    let address = server
        .server_addr()
        .to_ip()
        .context("no local review address")?;
    let origin = format!("http://{address}");
    let page_route = format!("/{}/", Uuid::new_v4().simple());
    let submit_route = format!("{page_route}selection");
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    status(json!({
        "status": "waiting",
        "url": format!("{origin}{page_route}"),
        "expires_in_seconds": timeout_seconds,
    }))?;
    while !cancel.load(Ordering::Relaxed) && Instant::now() < deadline {
        let Some(mut request) = server.recv_timeout(Duration::from_millis(250))? else {
            continue;
        };
        if request.method() == &Method::Get && request.url() == page_route {
            let _ = respond(request, 200, "text/html; charset=utf-8", &html);
            continue;
        }
        if request.method() != &Method::Post || request.url() != submit_route {
            let _ = respond(
                request,
                404,
                "text/plain; charset=utf-8",
                "Review page not found",
            );
            continue;
        }
        if !request
            .headers()
            .iter()
            .any(|header| header.field.equiv("Origin") && header.value.as_str() == origin.as_str())
        {
            let _ = respond(
                request,
                403,
                "text/plain; charset=utf-8",
                "Open the local review link to submit",
            );
            continue;
        }
        let Ok(selection) =
            read_submission(&mut request).and_then(|submission| manifest.select(submission))
        else {
            let _ = respond(
                request,
                400,
                "text/plain; charset=utf-8",
                "Invalid review selection",
            );
            continue;
        };
        let (id, path) = match store.write("selection", &selection) {
            Ok(saved) => saved,
            Err(error) => {
                let _ = respond(
                    request,
                    500,
                    "text/plain; charset=utf-8",
                    "Could not save the review selection",
                );
                return Err(error);
            }
        };
        let _ = respond(
            request,
            200,
            "text/plain; charset=utf-8",
            "Selection received",
        );
        status(json!({
            "status": "submitted",
            "selection_id": id,
            "selection_file": path,
            "selected": selection.choices.len(),
            "scan_ids": selection.scan_ids,
            "include_groups": selection.include_groups,
        }))?;
        return Ok(());
    }
    let outcome = if cancel.load(Ordering::Relaxed) {
        "cancelled"
    } else {
        "expired"
    };
    status(json!({"status": outcome}))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_keeps_clues_and_builds_preview_scope_from_eligible_yellow_choices() {
        let choice = |level, group: Option<&str>, scan: Option<&str>| Choice {
            level,
            label: String::new(),
            group_id: group.map(str::to_owned),
            scan_ids: scan.into_iter().map(str::to_owned).collect(),
            findings: Vec::new(),
        };
        let manifest = Manifest {
            schema_version: 1,
            choices: vec![
                choice(
                    ReviewLevel::NeedsReview,
                    Some("chrome/web/resources"),
                    Some("scan-a"),
                ),
                choice(ReviewLevel::NeedsReview, Some("chrome/web/scripts"), None),
                choice(ReviewLevel::Suspected, None, None),
                choice(
                    ReviewLevel::NeedsReview,
                    Some("other/web/resources"),
                    Some("scan-b"),
                ),
            ],
        };
        let selection = manifest
            .select(Submission {
                choices: BTreeSet::from([0, 1, 2]),
            })
            .unwrap();
        assert_eq!(selection.scan_ids, BTreeSet::from(["scan-a"]));
        assert_eq!(
            selection.include_groups,
            BTreeSet::from(["chrome/web/resources"])
        );
        assert_eq!(selection.choices.len(), 3);
        assert_eq!(selection.choices[2].level, ReviewLevel::Suspected);
        assert!(
            manifest
                .select(Submission {
                    choices: BTreeSet::from([4])
                })
                .is_err()
        );
    }
}
