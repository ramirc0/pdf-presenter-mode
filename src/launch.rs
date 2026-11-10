//! What to present and where, from the command line or the start page.

use std::path::PathBuf;

use crate::deck::{self, Deck};
use crate::presenter::Projector;
use crate::timer::Goal;

/// A talk ready to start: the deck, its notes, and where each window goes.
#[derive(Clone)]
pub struct Launch {
    /// The PDF to present.
    pub deck: PathBuf,
    /// A pdfpc file whose notes replace the deck's own.
    pub notes: Option<PathBuf>,
    /// Where the audience window goes.
    pub audience: Projector,
    /// The monitor the presenter goes fullscreen on, or `None` for a normal window.
    pub presenter: Option<String>,
    /// A countdown that replaces the notes file's.
    pub goal: Option<Goal>,
}

impl Launch {
    /// Opens the deck and its notes, with `goal` in place of the notes file's countdown.
    ///
    /// # Errors
    ///
    /// Fails as [`Deck::open`] does.
    pub fn open(&self) -> anyhow::Result<Deck> {
        let mut deck = Deck::open(&self.deck, self.notes.as_deref())?;
        if self.goal.is_some() {
            deck.notes.schedule.goal = self.goal;
        }
        Ok(deck)
    }

    /// The files whose changes reload the deck: the PDF and its notes file.
    pub fn watched(&self) -> Vec<PathBuf> {
        let notes = self.notes.clone();
        vec![
            self.deck.clone(),
            notes.unwrap_or_else(|| deck::sidecar(&self.deck)),
        ]
    }
}
