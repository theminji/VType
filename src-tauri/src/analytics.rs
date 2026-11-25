use chrono::{Days, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::ErrorKind, path::Path, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager};
use unicode_segmentation::UnicodeSegmentation;

static HISTORY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dictation {
    raw_text: String,
    recording_ms: u64,
    processing_ms: u64,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Totals {
    dictations: u64,
    spoken_words: u64,
    filler_words: u64,
    inserted_words: u64,
    recording_ms: u64,
    processing_ms: u64,
}

impl Totals {
    fn add(&mut self, other: &Self) {
        self.dictations += other.dictations;
        self.spoken_words += other.spoken_words;
        self.filler_words += other.filler_words;
        self.inserted_words += other.inserted_words;
        self.recording_ms += other.recording_ms;
        self.processing_ms += other.processing_ms;
    }

    fn from_dictation(dictation: Dictation, inserted: &str, insertion_ms: u64) -> Self {
        let words = dictation.raw_text.unicode_words();
        let mut spoken_words = 0;
        let mut filler_words = 0;
        for word in words {
            spoken_words += 1;
            if matches!(
                word.to_ascii_lowercase().as_str(),
                "um" | "uh" | "erm" | "er" | "hmm" | "mm" | "umm" | "uhm"
            ) {
                filler_words += 1;
            }
        }
        Self {
            dictations: 1,
            spoken_words,
            filler_words,
            inserted_words: inserted.unicode_words().count() as u64,
            recording_ms: dictation.recording_ms,
            processing_ms: dictation.processing_ms.saturating_add(insertion_ms),
        }
    }
}

type History = BTreeMap<NaiveDate, Totals>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityDay {
    date: NaiveDate,
    spoken_words: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analytics {
    totals: Totals,
    activity: Vec<ActivityDay>,
    typing_speed_wpm: u16,
}

fn read_history(path: &Path) -> Result<History, String> {
    match fs::read(path) {
        Ok(data) => serde_json::from_slice(&data).map_err(|err| err.to_string()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(History::new()),
        Err(err) => Err(err.to_string()),
    }
}

fn append(path: &Path, date: NaiveDate, entry: &Totals) -> Result<(), String> {
    let directory = path.parent().ok_or("Analytics directory is unavailable.")?;
    fs::create_dir_all(directory).map_err(|err| err.to_string())?;
    let mut history = read_history(path)?;
    history.entry(date).or_default().add(entry);
    let mut next = tempfile::NamedTempFile::new_in(directory).map_err(|err| err.to_string())?;
    serde_json::to_writer(&mut next, &history).map_err(|err| err.to_string())?;
    next.persist(path).map_err(|err| err.to_string())?;
    Ok(())
}

pub fn record(
    app: &AppHandle,
    dictation: Dictation,
    inserted: &str,
    insertion_ms: u64,
) -> Result<(), String> {
    let entry = Totals::from_dictation(dictation, inserted, insertion_ms);
    let path = app
        .path()
        .app_local_data_dir()
        .map_err(|err| err.to_string())?
        .join("analytics.json");
    let guard = HISTORY_LOCK
        .lock()
        .map_err(|_| "Analytics are unavailable.")?;
    append(&path, Local::now().date_naive(), &entry)?;
    drop(guard);
    let _ = app.emit_to("settings", "analytics-changed", ());
    Ok(())
}

fn summarize(history: &History, today: NaiveDate, typing_speed_wpm: u16) -> Analytics {
    let mut totals = Totals::default();
    for entry in history.values() {
        totals.add(entry);
    }
    let activity = (0..7)
        .rev()
        .map(|offset| {
            let date = today - Days::new(offset);
            ActivityDay {
                date,
                spoken_words: history.get(&date).map_or(0, |row| row.spoken_words),
            }
        })
        .collect();
    Analytics {
        totals,
        activity,
        typing_speed_wpm,
    }
}

#[tauri::command]
pub async fn get_analytics(app: AppHandle) -> Result<Analytics, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let typing_speed = super::settings::snapshot(&app)?.typing_speed_wpm;
        let path = app
            .path()
            .app_local_data_dir()
            .map_err(|err| err.to_string())?
            .join("analytics.json");
        let _guard = HISTORY_LOCK
            .lock()
            .map_err(|_| "Analytics are unavailable.")?;
        let history = read_history(&path)?;
        Ok(summarize(&history, Local::now().date_naive(), typing_speed))
    })
    .await
    .map_err(|err| err.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_only_aggregates_and_keeps_calendar_gaps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("analytics.json");
        let today = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
        let entry = Totals::from_dictation(
            Dictation {
                raw_text: "Um, voilà, uh, two words.".into(),
                recording_ms: 2200,
                processing_ms: 800,
            },
            "Voilà, two words.",
            200,
        );
        append(&path, today, &entry).unwrap();
        append(&path, today - Days::new(8), &entry).unwrap();
        let analytics = summarize(&read_history(&path).unwrap(), today, 60);
        assert_eq!(analytics.totals.dictations, 2);
        assert_eq!(analytics.totals.spoken_words, 10);
        assert_eq!(analytics.totals.filler_words, 4);
        assert_eq!(analytics.totals.inserted_words, 6);
        assert_eq!(analytics.totals.processing_ms, 2000);
        assert_eq!(analytics.activity.len(), 7);
        assert_eq!(analytics.activity[5].spoken_words, 0);
        assert_eq!(analytics.activity[6].spoken_words, 5);
        assert!(!fs::read_to_string(path).unwrap().contains("Voilà"));
    }
}
