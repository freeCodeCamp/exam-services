//! Version-comparison harness for the moderation-score algorithm.
//!
//! Runs every [`crate::moderation_versions::VERSIONS`] entry over a committed,
//! PII-free synthetic scenario catalog (`testdata/moderation/scenarios/*.json`)
//! and produces two artifacts from the one score matrix:
//!
//! - **Regression gate** ([`moderation_scores_golden`]): asserts the matrix
//!   matches `testdata/moderation/scores.golden`. Any change to a live version's
//!   output (or an accidental edit to a frozen version) fails CI. Regenerate the
//!   golden deliberately with `UPDATE_GOLDEN=1`.
//! - **Report view** ([`moderation_report_html`]): renders a self-contained,
//!   sortable HTML page comparing every version per scenario, with deltas and
//!   moderation-threshold crossings. Uploaded by CI as a downloadable artifact.
//!
//! When a local (git-ignored) `../fixtures/` dump is present, the report also
//! recomputes every version over the real attempts as an extra section; CI has
//! no fixtures, so that section is simply empty.
//!
//! Scenarios are authored in a compact schema (not full prisma extended-JSON);
//! [`Scenario::build`] expands each into an `Attempt` + `Vec<Event>` using the
//! same deterministic `oid`/`bdt` conventions as the `attempt::logic` unit tests.

use std::path::Path;

use bson::{DateTime, oid::ObjectId};
use prisma::{
    ExamEnvironmentConfig, ExamEnvironmentExam, ExamEnvironmentExamAttempt,
    ExamEnvironmentGeneratedExam,
    supabase::{Event, EventKind},
};
use serde::Deserialize;

use crate::attempt::{Attempt, AttemptQuestionSet, AttemptQuestionSetQuestion, construct_attempt};
use crate::moderation_versions::{LIVE, VERSIONS};

/// Fixed epoch offset - keeps timestamps clear of the Unix epoch so nothing
/// saturates against it. Matches `attempt::logic`'s `T0`.
const T0: i64 = 1_700_000_000_000;

/// Moderation threshold: at/above => flagged for manual review. Mirrors
/// `moderation-service`'s `MODERATION_THRESHOLD` default (and `attempt.rs`).
const THRESHOLD: f64 = 0.25;

const SCENARIOS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/moderation/scenarios");
const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/testdata/moderation/scores.golden"
);
const REPORT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/testdata/moderation/report.html"
);
/// Workspace-root `fixtures/` (optional local enrichment; git-ignored).
const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures");

// ---- deterministic builders (mirrors attempt::logic) ----------------------

fn oid(n: u8) -> ObjectId {
    let mut bytes = [0u8; 12];
    bytes[11] = n;
    ObjectId::from_bytes(bytes)
}

fn bdt(ms: i64) -> DateTime {
    DateTime::from_millis(T0 + ms)
}

fn ev(kind: EventKind, question: ObjectId, ms: i64) -> Event {
    Event {
        id: String::from("evt"),
        timestamp: DateTime::from_millis(T0 + ms).to_chrono(),
        kind,
        meta: serde_json::json!({ "question": question.to_hex() }),
        attempt_id: oid(0),
    }
}

fn q(id: ObjectId, submission_ms: Option<i64>) -> AttemptQuestionSetQuestion {
    AttemptQuestionSetQuestion {
        id,
        text: String::new(),
        tags: vec![],
        deprecated: false,
        audio: None,
        answers: vec![],
        selected: vec![],
        generated: vec![],
        submission_time: submission_ms.map(bdt),
    }
}

fn attempt_with(
    start_ms: i64,
    total_time_in_s: i64,
    passing_percent: f64,
    questions: Vec<AttemptQuestionSetQuestion>,
) -> Attempt {
    Attempt {
        id: oid(250),
        exam_id: oid(251),
        user_id: oid(252),
        prerequisites: vec![],
        deprecated: false,
        question_sets: vec![AttemptQuestionSet {
            id: oid(253),
            _type: Default::default(),
            context: None,
            questions,
        }],
        config: ExamEnvironmentConfig {
            total_time_in_s,
            passing_percent,
            ..Default::default()
        },
        start_time: bdt(start_ms),
    }
}

// ---- compact scenario schema ----------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scenario {
    name: String,
    description: String,
    total_time_in_s: i64,
    passing_percent: f64,
    #[serde(default)]
    start_time_ms: i64,
    questions: Vec<SQuestion>,
    #[serde(default)]
    events: Vec<SEvent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SQuestion {
    /// Omitted => question was never submitted.
    #[serde(default)]
    submission_ms: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SEvent {
    kind: EventKind,
    /// 1-based index into `questions`.
    question: u8,
    at_ms: i64,
}

impl Scenario {
    fn build(&self) -> (Attempt, Vec<Event>) {
        let questions = self
            .questions
            .iter()
            .enumerate()
            .map(|(i, sq)| q(oid((i + 1) as u8), sq.submission_ms))
            .collect();
        let attempt = attempt_with(
            self.start_time_ms,
            self.total_time_in_s,
            self.passing_percent,
            questions,
        );
        let events = self
            .events
            .iter()
            .map(|se| ev(se.kind.clone(), oid(se.question), se.at_ms))
            .collect();
        (attempt, events)
    }
}

// ---- score matrix ----------------------------------------------------------

/// One row of the comparison matrix: a name/description plus one score cell per
/// registered version (aligned to [`VERSIONS`] order).
struct Case {
    name: String,
    description: String,
    cells: Vec<Result<f64, String>>,
}

/// Run every registered version over one attempt.
fn score_all(attempt: &Attempt, events: &Vec<Event>) -> Vec<Result<f64, String>> {
    VERSIONS
        .iter()
        .map(|v| (v.score)(attempt, events).map_err(|e| e.to_string()))
        .collect()
}

fn load_scenarios() -> Vec<Scenario> {
    let dir =
        std::fs::read_dir(SCENARIOS_DIR).unwrap_or_else(|e| panic!("read {SCENARIOS_DIR}: {e}"));
    let mut out = vec![];
    for entry in dir {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let scenario: Scenario = serde_json::from_slice(&bytes)
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        out.push(scenario);
    }
    // Sort by name so the matrix order is stable regardless of filesystem order.
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn synthetic_cases() -> Vec<Case> {
    load_scenarios()
        .iter()
        .map(|s| {
            let (attempt, events) = s.build();
            Case {
                name: s.name.clone(),
                description: s.description.clone(),
                cells: score_all(&attempt, &events),
            }
        })
        .collect()
}

/// Recompute every version over the local (git-ignored) real-attempt dump, if
/// present. Returns empty when `../fixtures/attempt` is absent (e.g. in CI).
fn real_cases() -> Vec<Case> {
    let attempt_dir = Path::new(FIXTURES_DIR).join("attempt");
    if !attempt_dir.exists() {
        return vec![];
    }
    let path_of = |dir: &str, id: &ObjectId| Path::new(FIXTURES_DIR).join(dir).join(id.to_hex());

    let mut cases = vec![];
    for entry in std::fs::read_dir(&attempt_dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(attempt) = serde_json::from_slice::<ExamEnvironmentExamAttempt>(&bytes) else {
            continue;
        };
        // Supporting fixtures may be absent for stale records.
        if !path_of("exam", &attempt.exam_id).exists()
            || !path_of("generation", &attempt.generated_exam_id).exists()
            || !path_of("events", &attempt.id).exists()
        {
            continue;
        }
        let exam: ExamEnvironmentExam =
            serde_json::from_slice(&std::fs::read(path_of("exam", &attempt.exam_id)).unwrap())
                .unwrap();
        let generation: ExamEnvironmentGeneratedExam = serde_json::from_slice(
            &std::fs::read(path_of("generation", &attempt.generated_exam_id)).unwrap(),
        )
        .unwrap();
        let events: Vec<Event> =
            serde_json::from_slice(&std::fs::read(path_of("events", &attempt.id)).unwrap())
                .unwrap();

        let constructed = construct_attempt(&exam, &generation, &attempt);
        cases.push(Case {
            name: attempt.id.to_hex(),
            description: String::new(),
            cells: score_all(&constructed, &events),
        });
    }
    // Most-flagged first (by live version); error rows sink to the bottom.
    let key = |c: &Case| live_score(c).unwrap_or(f64::NEG_INFINITY);
    cases.sort_by(|a, b| key(b).partial_cmp(&key(a)).unwrap());
    cases
}

/// Column index of the live version within a [`Case`]'s cells.
fn live_index() -> usize {
    VERSIONS
        .iter()
        .position(|v| v.id == LIVE)
        .expect("LIVE id must be registered in VERSIONS")
}

/// Score under the live version, if it did not error.
fn live_score(c: &Case) -> Option<f64> {
    c.cells
        .get(live_index())
        .and_then(|r| r.as_ref().ok().copied())
}

/// Score under the oldest (first) registered version, if it did not error.
fn oldest_score(c: &Case) -> Option<f64> {
    c.cells.first().and_then(|r| r.as_ref().ok().copied())
}

/// Threshold-band change between the oldest and the live version.
fn crossing(c: &Case) -> Option<&'static str> {
    match (oldest_score(c), live_score(c)) {
        (Some(o), Some(n)) => match (o >= THRESHOLD, n >= THRESHOLD) {
            (false, true) => Some("NOW-FLAGGED"),
            (true, false) => Some("NOW-CLEARED"),
            _ => None,
        },
        _ => None,
    }
}

// ---- golden gate -----------------------------------------------------------

fn cell_text(cell: &Result<f64, String>) -> String {
    match cell {
        Ok(x) => format!("{x:.4}"),
        Err(e) => format!("ERR:{e}"),
    }
}

fn golden_text(cases: &[Case]) -> String {
    let mut s = String::new();
    s.push_str("# Moderation-score golden - DO NOT hand-edit.\n");
    s.push_str("# Regenerate: UPDATE_GOLDEN=1 cargo test -p exam-utils moderation_scores_golden\n");
    s.push_str("# Format: <scenario> | <version> | <score|ERR:msg>\n");
    for case in cases {
        for (version, cell) in VERSIONS.iter().zip(&case.cells) {
            s.push_str(&format!(
                "{} | {} | {}\n",
                case.name,
                version.label,
                cell_text(cell)
            ));
        }
    }
    s
}

/// Key-aware diff of two golden texts (ignores comments), so a drift report
/// reads as `scenario | version: old -> new` rather than a raw line diff.
fn golden_diff(expected: &str, actual: &str) -> String {
    use std::collections::BTreeMap;
    let parse = |s: &str| -> BTreeMap<String, String> {
        s.lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                l.rsplit_once(" | ")
                    .map(|(k, v)| (k.trim().into(), v.trim().into()))
            })
            .collect()
    };
    let (e, a) = (parse(expected), parse(actual));
    let mut keys: Vec<&String> = e.keys().chain(a.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut out = String::new();
    for k in keys {
        match (e.get(k), a.get(k)) {
            (Some(x), Some(y)) if x != y => out.push_str(&format!("  {k}: {x} -> {y}\n")),
            (None, Some(y)) => out.push_str(&format!("  {k}: (new) {y}\n")),
            (Some(x), None) => out.push_str(&format!("  {k}: {x} (removed)\n")),
            _ => {}
        }
    }
    out
}

/// Regression gate. Recomputes the score matrix and asserts it matches the
/// committed golden. `UPDATE_GOLDEN=1` rewrites the golden instead of asserting.
#[test]
fn moderation_scores_golden() {
    let cases = synthetic_cases();
    let text = golden_text(&cases);

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::write(GOLDEN_PATH, &text).unwrap();
        eprintln!("UPDATE_GOLDEN: wrote {GOLDEN_PATH}");
        return;
    }

    let expected = std::fs::read_to_string(GOLDEN_PATH).unwrap_or_else(|e| {
        panic!("read golden ({e}); create it with UPDATE_GOLDEN=1 cargo test -p exam-utils moderation_scores_golden")
    });

    assert!(
        text == expected,
        "moderation scores drifted from golden. If intended, regenerate with \
         UPDATE_GOLDEN=1 and review the diff.\n\ndrift:\n{}",
        golden_diff(&expected, &text)
    );
}

// ---- HTML report -----------------------------------------------------------

/// Renders the score matrix to a self-contained HTML file (no external assets).
/// Not an assertion test - it always writes `report.html` so CI can upload it
/// and developers can open it locally.
#[test]
fn moderation_report_html() {
    let synthetic = synthetic_cases();
    let real = real_cases();
    let html = render_html(&synthetic, &real);
    std::fs::write(REPORT_PATH, html).unwrap();
    eprintln!("wrote {REPORT_PATH}");
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// One `<tr>` for a case: name, description, per-version score cells (with a
/// proportional bar + flag tint), delta, and crossing badge.
fn render_row(case: &Case, with_desc: bool) -> String {
    let mut tds = String::new();
    tds.push_str(&format!("<td class=\"name\">{}</td>", esc(&case.name)));
    if with_desc {
        tds.push_str(&format!(
            "<td class=\"desc\">{}</td>",
            esc(&case.description)
        ));
    }
    for cell in &case.cells {
        match cell {
            Ok(x) => {
                let pct = (x * 100.0).clamp(0.0, 100.0);
                let flagged = if *x >= THRESHOLD { " flag" } else { "" };
                tds.push_str(&format!(
                    "<td class=\"score{flagged}\" data-sort=\"{x}\">\
                       <span class=\"bar\" style=\"width:{pct:.1}%\"></span>\
                       <span class=\"val\">{x:.4}</span></td>"
                ));
            }
            Err(e) => tds.push_str(&format!(
                "<td class=\"err\" data-sort=\"-1\" title=\"{}\">ERR</td>",
                esc(e)
            )),
        }
    }
    // Delta (live - oldest) + crossing.
    let (delta_cell, cross_cell) = match (oldest_score(case), live_score(case)) {
        (Some(o), Some(n)) => {
            let d = n - o;
            let badge = match crossing(case) {
                Some(c) => format!("<span class=\"cross {}\">{c}</span>", c.to_lowercase()),
                None => String::new(),
            };
            (
                format!("<td class=\"delta\" data-sort=\"{d}\">{d:+.4}</td>"),
                format!("<td class=\"crossing\">{badge}</td>"),
            )
        }
        _ => (
            "<td class=\"delta\" data-sort=\"-999\">-</td>".to_string(),
            "<td class=\"crossing\"></td>".to_string(),
        ),
    };
    tds.push_str(&delta_cell);
    tds.push_str(&cross_cell);
    format!("<tr>{tds}</tr>")
}

fn render_table(id: &str, cases: &[Case], with_desc: bool) -> String {
    let mut ths = String::from("<th data-type=\"text\">scenario</th>");
    if with_desc {
        ths.push_str("<th data-type=\"text\">description</th>");
    }
    for version in VERSIONS {
        let live = if version.id == LIVE { " (live)" } else { "" };
        ths.push_str(&format!(
            "<th data-type=\"num\">{}{live}</th>",
            esc(version.label)
        ));
    }
    ths.push_str(&format!(
        "<th data-type=\"num\">&Delta; (live&minus;{})</th><th data-type=\"text\">crossing</th>",
        esc(VERSIONS[0].label)
    ));

    let rows: String = cases.iter().map(|c| render_row(c, with_desc)).collect();
    format!(
        "<table id=\"{id}\" class=\"sortable\"><thead><tr>{ths}</tr></thead><tbody>{rows}</tbody></table>"
    )
}

/// Per-version summary: how many non-error cases each version flags, plus the
/// spread of live-vs-oldest deltas and threshold crossings.
fn render_summary(cases: &[Case]) -> String {
    let mut cards = String::new();

    for (i, version) in VERSIONS.iter().enumerate() {
        let scores: Vec<f64> = cases
            .iter()
            .filter_map(|c| c.cells.get(i).and_then(|r| r.as_ref().ok().copied()))
            .collect();
        let errors = cases.len() - scores.len();
        let flagged = scores.iter().filter(|s| **s >= THRESHOLD).count();
        let mean = if scores.is_empty() {
            0.0
        } else {
            scores.iter().sum::<f64>() / scores.len() as f64
        };
        let state = if version.id == LIVE { "live" } else { "frozen" };
        cards.push_str(&format!(
            "<div class=\"card\"><h3>{} <span class=\"muted\">{state} &middot; {}</span></h3>\
               <div class=\"stat\"><b>{flagged}</b><span>flagged (&ge;{THRESHOLD})</span></div>\
               <div class=\"stat\"><b>{:.3}</b><span>mean score</span></div>\
               <div class=\"stat\"><b>{errors}</b><span>errors</span></div></div>",
            esc(version.label),
            esc(version.introduced_in),
            mean
        ));
    }

    // Crossings between the oldest and the live version.
    let now_flagged = cases
        .iter()
        .filter(|c| crossing(c) == Some("NOW-FLAGGED"))
        .count();
    let now_cleared = cases
        .iter()
        .filter(|c| crossing(c) == Some("NOW-CLEARED"))
        .count();
    let deltas: Vec<f64> = cases
        .iter()
        .filter_map(|c| match (oldest_score(c), live_score(c)) {
            (Some(o), Some(n)) => Some(n - o),
            _ => None,
        })
        .collect();
    let (dmean, dmax, dmin) = if deltas.is_empty() {
        (0.0, 0.0, 0.0)
    } else {
        let mean = deltas.iter().sum::<f64>() / deltas.len() as f64;
        let max = deltas.iter().cloned().fold(f64::MIN, f64::max);
        let min = deltas.iter().cloned().fold(f64::MAX, f64::min);
        (mean, max, min)
    };
    cards.push_str(&format!(
        "<div class=\"card wide\"><h3>live vs oldest</h3>\
           <div class=\"stat\"><b>{now_flagged}</b><span>now flagged</span></div>\
           <div class=\"stat\"><b>{now_cleared}</b><span>now cleared</span></div>\
           <div class=\"stat\"><b>{dmean:+.3}</b><span>&Delta; mean</span></div>\
           <div class=\"stat\"><b>{dmin:+.3} / {dmax:+.3}</b><span>&Delta; min / max</span></div></div>"
    ));

    format!("<div class=\"summary\">{cards}</div>")
}

fn render_html(synthetic: &[Case], real: &[Case]) -> String {
    let versions_line = VERSIONS
        .iter()
        .map(|v| {
            if v.id == LIVE {
                format!("{} (live)", v.label)
            } else {
                v.label.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" &middot; ");

    let real_section = if real.is_empty() {
        String::from(
            "<p class=\"muted\">No local <code>fixtures/</code> dump found - run \
             <code>dump_moderation_fixtures</code> then re-run this test to compare every \
             version over real attempts here.</p>",
        )
    } else {
        format!(
            "<p class=\"muted\">{} real attempts from the local <code>fixtures/</code> dump \
             (git-ignored, not in CI), most-flagged first.</p>{}",
            real.len(),
            render_table("real", real, false)
        )
    };

    format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Moderation-score version comparison</title>
<style>
  :root {{
    --bg:#fff; --fg:#1a1a1a; --muted:#666; --line:#e2e2e2;
    --bar:#cfe8d4; --flag:#f6cccc; --flagfg:#a11; --head:#f4f4f5;
  }}
  @media (prefers-color-scheme: dark) {{
    :root {{
      --bg:#16171a; --fg:#e6e6e6; --muted:#9a9a9a; --line:#2c2e33;
      --bar:#294b34; --flag:#4a2626; --flagfg:#ff9a9a; --head:#202227;
    }}
  }}
  * {{ box-sizing:border-box; }}
  body {{ margin:0; padding:2rem; font:14px/1.5 system-ui,sans-serif; color:var(--fg); background:var(--bg); }}
  h1 {{ font-size:1.4rem; margin:0 0 .25rem; }}
  .sub {{ color:var(--muted); margin:0 0 1.5rem; }}
  .sub code {{ background:var(--head); padding:.1em .35em; border-radius:4px; }}
  h2 {{ font-size:1.1rem; margin:2rem 0 .5rem; border-bottom:1px solid var(--line); padding-bottom:.3rem; }}
  .muted {{ color:var(--muted); }}
  .summary {{ display:flex; flex-wrap:wrap; gap:.75rem; margin:.5rem 0 1rem; }}
  .card {{ border:1px solid var(--line); border-radius:8px; padding:.75rem 1rem; min-width:180px; }}
  .card.wide {{ min-width:280px; }}
  .card h3 {{ margin:0 0 .5rem; font-size:.85rem; font-weight:600; color:var(--muted); }}
  .stat {{ display:flex; align-items:baseline; gap:.5rem; }}
  .stat b {{ font-size:1.05rem; font-variant-numeric:tabular-nums; }}
  .stat span {{ color:var(--muted); font-size:.8rem; }}
  .scroll {{ overflow-x:auto; }}
  table {{ border-collapse:collapse; width:100%; font-variant-numeric:tabular-nums; }}
  th, td {{ text-align:left; padding:.4rem .6rem; border-bottom:1px solid var(--line); white-space:nowrap; }}
  thead th {{ position:sticky; top:0; background:var(--head); cursor:pointer; user-select:none; }}
  thead th::after {{ content:" \2195"; color:var(--muted); font-size:.75em; }}
  td.desc {{ white-space:normal; min-width:22rem; color:var(--muted); font-size:.85rem; }}
  td.name {{ font-weight:600; }}
  td.score {{ position:relative; text-align:right; min-width:6rem; }}
  td.score .bar {{ position:absolute; left:0; top:0; bottom:0; background:var(--bar); z-index:0; }}
  td.score.flag .bar {{ background:var(--flag); }}
  td.score .val {{ position:relative; z-index:1; }}
  td.score.flag .val {{ color:var(--flagfg); font-weight:600; }}
  td.delta {{ text-align:right; }}
  td.err {{ color:var(--flagfg); font-weight:600; }}
  .cross {{ font-size:.7rem; font-weight:700; padding:.1em .45em; border-radius:999px; }}
  .cross.now-flagged {{ background:var(--flag); color:var(--flagfg); }}
  .cross.now-cleared {{ background:var(--bar); }}
</style></head>
<body>
  <h1>Moderation-score version comparison</h1>
  <p class="sub">Versions: {versions_line} &nbsp;&middot;&nbsp; threshold {THRESHOLD} &nbsp;&middot;&nbsp; click any header to sort.<br>
  Bars are score magnitude; tinted red at/above threshold. &Delta; and crossings compare the live version to the oldest.</p>

  <h2>Synthetic scenarios <span class="muted">(committed, CI-gated)</span></h2>
  {summary}
  <div class="scroll">{synthetic_table}</div>

  <h2>Real attempts <span class="muted">(local only)</span></h2>
  {real_section}

<script>
  document.querySelectorAll('table.sortable').forEach(function (table) {{
    var head = table.tHead.rows[0];
    Array.prototype.forEach.call(head.cells, function (th, col) {{
      var dir = 1;
      th.addEventListener('click', function () {{
        var num = th.dataset.type === 'num';
        var rows = Array.prototype.slice.call(table.tBodies[0].rows);
        rows.sort(function (a, b) {{
          var av = cellKey(a.cells[col], num), bv = cellKey(b.cells[col], num);
          return av < bv ? -dir : av > bv ? dir : 0;
        }});
        dir = -dir;
        rows.forEach(function (r) {{ table.tBodies[0].appendChild(r); }});
      }});
    }});
    function cellKey(td, num) {{
      var raw = td.dataset.sort !== undefined ? td.dataset.sort : td.textContent;
      return num ? parseFloat(raw) : raw.toLowerCase();
    }}
  }});
</script>
</body></html>
"##,
        summary = render_summary(synthetic),
        synthetic_table = render_table("synthetic", synthetic, true),
    )
}
