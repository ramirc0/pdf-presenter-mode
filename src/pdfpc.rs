//! Speaker notes in pdfpc format 2, the JSON that pdfpc and the Typst notes snippet write.

use std::collections::BTreeMap;

use anyhow::Context;
use serde::Deserialize;

use crate::timer::{Goal, Schedule};

/// The speaker notes of a deck, by page.
#[derive(Debug, Default, PartialEq)]
pub struct Notes {
    /// Note text by page index, counting from 0.
    pages: BTreeMap<usize, String>,
    /// Whether to show the notes as plain text instead of Markdown, from pdfpc's `disableMarkdown`.
    pub plain_text: bool,
    /// The countdown, from pdfpc's `duration` or `endTime`, and `lastMinutes`.
    pub schedule: Schedule,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    #[serde(default)]
    disable_markdown: bool,
    /// Minutes.
    duration: Option<u64>,
    /// `HH:MM`, local time.
    end_time: Option<String>,
    last_minutes: Option<u32>,
    #[serde(default)]
    pages: Vec<Page>,
}

#[derive(Deserialize)]
struct Page {
    idx: usize,
    #[serde(default)]
    note: String,
}

impl Notes {
    /// Parses a pdfpc format 2 file. Keys this app does not use are ignored.
    ///
    /// Entries that share a page are joined in file order, separated by a blank line.
    /// `endTime` wins over `duration`, since it is the harder limit.
    ///
    /// # Errors
    ///
    /// Fails if `json` is not a JSON object, a known key has the wrong type,
    /// or `endTime` is not a time such as `14:30`.
    pub fn from_pdfpc(json: &[u8]) -> anyhow::Result<Self> {
        let file: File = serde_json::from_slice(json)?;
        let end_time = file
            .end_time
            .map(|end| {
                end.parse()
                    .with_context(|| format!("endTime {end:?} is not a time such as 14:30"))
            })
            .transpose()?;
        let goal = end_time.map(Goal::EndsAt).or(file
            .duration
            .map(|minutes| Goal::Length(std::time::Duration::from_secs(minutes * 60))));
        let schedule = Schedule {
            goal,
            last_minutes: file
                .last_minutes
                .unwrap_or(Schedule::default().last_minutes),
        };
        let mut pages: BTreeMap<usize, String> = BTreeMap::new();
        for page in file.pages.into_iter().filter(|page| !page.note.is_empty()) {
            pages
                .entry(page.idx)
                .and_modify(|text| {
                    text.push_str("\n\n");
                    text.push_str(&page.note);
                })
                .or_insert(page.note);
        }
        Ok(Self {
            pages,
            plain_text: file.disable_markdown,
            schedule,
        })
    }

    /// The note for page `index`, counting from 0, if it has one.
    pub fn on_page(&self, index: usize) -> Option<&str> {
        self.pages.get(&index).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_notes_by_page() {
        let notes = Notes::from_pdfpc(
            br#"{"pdfpcFormat": 2, "pages": [
                {"idx": 1, "label": "2", "overlay": 0, "note": "Second page."},
                {"idx": 3, "label": "4", "overlay": 0, "note": "Fourth page."}]}"#,
        )
        .unwrap();
        assert_eq!(notes.on_page(1), Some("Second page."));
        assert_eq!(notes.on_page(3), Some("Fourth page."));
        assert_eq!(notes.on_page(0), None);
        assert!(!notes.plain_text);
    }

    #[test]
    fn joins_notes_that_share_a_page() {
        let notes = Notes::from_pdfpc(
            br#"{"pages": [{"idx": 0, "note": "First."}, {"idx": 0, "note": "Second."}]}"#,
        )
        .unwrap();
        assert_eq!(notes.on_page(0), Some("First.\n\nSecond."));
    }

    #[test]
    fn ignores_unknown_keys() {
        let notes = Notes::from_pdfpc(
            br#"{"pdfpcFormat": 2, "endTime": "10:30", "future": {"x": 1},
                "pages": [{"idx": 0, "hidden": false, "extra": [1], "note": "Kept."}]}"#,
        )
        .unwrap();
        assert_eq!(notes.on_page(0), Some("Kept."));
    }

    #[test]
    fn honors_disable_markdown() {
        let notes = Notes::from_pdfpc(br#"{"disableMarkdown": true, "pages": []}"#).unwrap();
        assert!(notes.plain_text);
    }

    #[test]
    fn skips_entries_without_a_note() {
        let notes = Notes::from_pdfpc(br#"{"pages": [{"idx": 0, "label": "1"}]}"#).unwrap();
        assert_eq!(notes, Notes::from_pdfpc(b"{}").unwrap());
    }

    #[test]
    fn reads_the_schedule() {
        let notes =
            Notes::from_pdfpc(br#"{"duration": 20, "lastMinutes": 3, "pages": []}"#).unwrap();
        assert_eq!(
            notes.schedule.goal,
            Some(Goal::Length(std::time::Duration::from_secs(1200)))
        );
        assert_eq!(notes.schedule.last_minutes, 3);
        let notes = Notes::from_pdfpc(br#"{"duration": 20, "endTime": "14:30"}"#).unwrap();
        assert_eq!(
            notes.schedule.goal,
            Some(Goal::EndsAt(jiff::civil::Time::constant(14, 30, 0, 0)))
        );
        assert_eq!(notes.schedule.last_minutes, 5);
    }

    #[test]
    fn rejects_a_bad_end_time() {
        let err = Notes::from_pdfpc(br#"{"endTime": "half past two"}"#).unwrap_err();
        assert!(err.to_string().contains("endTime"), "{err}");
    }

    #[test]
    fn rejects_a_wrong_type() {
        assert!(Notes::from_pdfpc(br#"{"pages": [{"idx": "one"}]}"#).is_err());
    }
}
