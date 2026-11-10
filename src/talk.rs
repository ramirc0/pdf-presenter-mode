//! The state of a running talk, free of any GUI.

use std::time::Instant;

use crate::settings::Settings;
use crate::timer::Timer;

/// Something the speaker asks for, from a key or a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Go to the next slide.
    Next,
    /// Go to the previous slide.
    Previous,
    /// Go to the next slide, or down a row in the overview.
    Down,
    /// Go to the previous slide, or up a row in the overview.
    Up,
    /// Open or close the overview of all slides.
    Overview,
    /// Go to this slide, picked with the mouse in the overview.
    Pick(usize),
    /// Go to the first slide.
    First,
    /// Go to the last slide.
    Last,
    /// Start typing a slide number to go to.
    GoTo,
    /// A digit of the slide number being typed.
    Digit(u8),
    /// Delete the last digit typed.
    Backspace,
    /// Go to the slide number typed.
    Confirm,
    /// Swap what the two windows show.
    Swap,
    /// Turn the audience view black, or back.
    Blank,
    /// Keep the audience on the current slide while the presenter moves on, or let it follow again.
    Freeze,
    /// Show the mouse on the current slide to the audience as a laser dot that draws when dragged, or stop.
    Laser,
    /// Erase the ink on the current slide.
    Erase,
    /// Start or resume the timer.
    StartTimer,
    /// Pause or resume the timer.
    PauseTimer,
    /// Stop the timer and set it to zero.
    ResetTimer,
    /// Switch between the two presenter layouts.
    Layout,
    /// Make the notes larger.
    BiggerNotes,
    /// Make the notes smaller.
    SmallerNotes,
    /// Toggle fullscreen for the window the key went to.
    Fullscreen,
    /// Show or hide the shortcut hint.
    Help,
    /// Close whatever is open, such as the shortcut hint.
    Cancel,
    /// Ask to quit. It takes effect when asked twice in a row.
    Quit,
}

/// What the views must do after an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Keep presenting.
    Continue,
    /// Close the app.
    Quit,
}

/// Position in the deck, the window roles, what is open, and the pending quit request.
#[derive(Debug)]
pub struct Talk {
    slide: usize,
    slides: usize,
    swapped: bool,
    blanked: bool,
    /// The slide the audience stays on, while frozen.
    frozen: Option<usize>,
    laser: bool,
    /// The slide number typed so far after `g`, 0 before the first digit.
    goto: Option<usize>,
    /// The slide selected in the overview, while it is open.
    overview: Option<usize>,
    timer: Timer,
    settings: Settings,
    help: bool,
    quit_armed: bool,
}

impl Talk {
    /// Starts a talk on the first of `slides` slides, with the presenter preferences `settings`.
    pub fn new(slides: usize, settings: Settings) -> Self {
        Self {
            slide: 0,
            slides,
            swapped: false,
            blanked: false,
            frozen: None,
            laser: false,
            goto: None,
            overview: None,
            timer: Timer::default(),
            settings,
            help: false,
            quit_armed: false,
        }
    }

    /// Carries the talk over to a reloaded deck of `slides` slides.
    /// Positions past the new end move to the last slide.
    pub fn reload(&mut self, slides: usize) {
        self.slides = slides;
        let last = slides.saturating_sub(1);
        self.slide = self.slide.min(last);
        self.frozen = self.frozen.map(|slide| slide.min(last));
        self.overview = self
            .overview
            .filter(|_| slides > 0)
            .map(|slide| slide.min(last));
    }

    /// The zero-based index of the current slide.
    pub fn slide(&self) -> usize {
        self.slide
    }

    /// The number of slides in the deck.
    pub fn slides(&self) -> usize {
        self.slides
    }

    /// The slide after the current one, if any.
    pub fn next_slide(&self) -> Option<usize> {
        (self.slide + 1 < self.slides).then_some(self.slide + 1)
    }

    /// The slide the audience sees, or `None` while blanked or for an empty deck.
    pub fn audience_slide(&self) -> Option<usize> {
        if self.blanked || self.slides == 0 {
            return None;
        }
        Some(self.frozen.unwrap_or(self.slide))
    }

    /// The slide selected in the overview, while it is open.
    pub fn overview(&self) -> Option<usize> {
        self.overview
    }

    /// How many slides the overview puts in a row: about as many rows as columns,
    /// so 16:9 thumbnails fill a 16:9 window.
    pub fn overview_columns(&self) -> usize {
        let root = self.slides.isqrt();
        if root * root == self.slides {
            root
        } else {
            root + 1
        }
    }

    /// The talk's stopwatch.
    pub fn timer(&self) -> &Timer {
        &self.timer
    }

    /// The slide number being typed after `g`, if any. `Some(0)` before the first digit.
    pub fn goto(&self) -> Option<usize> {
        self.goto
    }

    /// Whether the audience view is black.
    pub fn blanked(&self) -> bool {
        self.blanked
    }

    /// Whether the audience stays on one slide while the presenter moves.
    pub fn frozen(&self) -> bool {
        self.frozen.is_some()
    }

    /// Whether the mouse on the current slide shows to the audience as a laser dot.
    pub fn laser(&self) -> bool {
        self.laser
    }

    /// Whether the presenter window shows the audience view and the other way around.
    pub fn swapped(&self) -> bool {
        self.swapped
    }

    /// The presenter preferences, changed by the layout and notes size keys.
    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// The presenter preferences, for dragging a split.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Whether the shortcut hint is open.
    pub fn help(&self) -> bool {
        self.help
    }

    /// Whether one more quit request ends the talk.
    pub fn quit_armed(&self) -> bool {
        self.quit_armed
    }

    /// Applies an action taken at `now` and says whether the app keeps running.
    /// The first slide change starts the timer.
    pub fn apply(&mut self, action: Action, now: Instant) -> Outcome {
        let before = self.slide;
        let outcome = self.change(action, now);
        if self.slide != before && self.timer.unused() {
            self.timer.start(now);
        }
        outcome
    }

    /// Handles `action` while the overview is open with `selected` selected.
    /// Returns `None` for actions that work the same with the overview open.
    fn browse(&mut self, selected: usize, action: Action) -> Option<Outcome> {
        let last = self.slides.saturating_sub(1);
        let columns = self.overview_columns();
        let select = match action {
            Action::Next => (selected + 1).min(last),
            Action::Previous => selected.saturating_sub(1),
            Action::Down if selected + columns <= last => selected + columns,
            Action::Up if selected >= columns => selected - columns,
            Action::Down | Action::Up => selected,
            Action::First => 0,
            Action::Last => last,
            Action::Confirm | Action::Pick(_) => {
                let target = match action {
                    Action::Pick(slide) => slide.min(last),
                    _ => selected,
                };
                self.overview = None;
                self.slide = target;
                return Some(Outcome::Continue);
            }
            Action::Overview | Action::Cancel => {
                self.overview = None;
                return Some(Outcome::Continue);
            }
            _ => return None,
        };
        self.overview = Some(select);
        Some(Outcome::Continue)
    }

    fn change(&mut self, action: Action, now: Instant) -> Outcome {
        if action == Action::Quit {
            if self.quit_armed {
                return Outcome::Quit;
            }
            self.quit_armed = true;
            return Outcome::Continue;
        }
        self.quit_armed = false;
        let last = self.slides.saturating_sub(1);
        if let Some(selected) = self.overview
            && let Some(outcome) = self.browse(selected, action)
        {
            return outcome;
        }
        if let Some(typed) = self.goto {
            match action {
                Action::Digit(digit) => {
                    let longer = typed * 10 + usize::from(digit);
                    if longer <= self.slides.max(9) {
                        self.goto = Some(longer);
                    }
                    return Outcome::Continue;
                }
                Action::Backspace => {
                    self.goto = Some(typed / 10);
                    return Outcome::Continue;
                }
                Action::Confirm => {
                    if typed > 0 {
                        self.slide = (typed - 1).min(last);
                    }
                    self.goto = None;
                    return Outcome::Continue;
                }
                _ => self.goto = None,
            }
        }
        match action {
            Action::Next | Action::Down => self.slide = (self.slide + 1).min(last),
            Action::Previous | Action::Up => self.slide = self.slide.saturating_sub(1),
            Action::Overview => self.overview = (self.slides > 0).then_some(self.slide),
            Action::Pick(slide) => self.slide = slide.min(last),
            Action::First => self.slide = 0,
            Action::Last => self.slide = last,
            Action::GoTo => self.goto = Some(0),
            Action::Digit(_) | Action::Backspace | Action::Confirm => {}
            Action::Swap => self.swapped = !self.swapped,
            Action::Blank => self.blanked = !self.blanked,
            Action::Freeze => {
                self.frozen = match self.frozen {
                    Some(_) => None,
                    None => Some(self.slide),
                }
            }
            Action::Laser => self.laser = !self.laser,
            Action::StartTimer => self.timer.start(now),
            Action::PauseTimer => self.timer.toggle_pause(now),
            Action::ResetTimer => self.timer.reset(),
            Action::Layout => self.settings.layout = self.settings.layout.other(),
            Action::BiggerNotes => self.settings.bigger_notes(),
            Action::SmallerNotes => self.settings.smaller_notes(),
            Action::Help => self.help = !self.help,
            Action::Cancel => {
                self.help = false;
                self.overview = None;
            }
            // The window that got the key toggles its own fullscreen.
            Action::Fullscreen => {}
            // The stage keeps the ink.
            Action::Erase => {}
            Action::Quit => unreachable!("handled above"),
        }
        Outcome::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_stays_inside_the_deck() {
        let mut talk = Talk::new(3, Settings::default());
        talk.apply(Action::Previous, Instant::now());
        assert_eq!(talk.slide(), 0);
        talk.apply(Action::Next, Instant::now());
        talk.apply(Action::Next, Instant::now());
        talk.apply(Action::Next, Instant::now());
        assert_eq!(talk.slide(), 2);
        assert_eq!(talk.next_slide(), None);
        talk.apply(Action::First, Instant::now());
        assert_eq!(talk.slide(), 0);
        assert_eq!(talk.next_slide(), Some(1));
        talk.apply(Action::Last, Instant::now());
        assert_eq!(talk.slide(), 2);
    }

    #[test]
    fn an_empty_deck_has_no_next_slide() {
        let mut talk = Talk::new(0, Settings::default());
        talk.apply(Action::Next, Instant::now());
        assert_eq!(talk.slide(), 0);
        assert_eq!(talk.next_slide(), None);
    }

    #[test]
    fn swap_toggles_and_keeps_the_slide() {
        let mut talk = Talk::new(3, Settings::default());
        talk.apply(Action::Next, Instant::now());
        talk.apply(Action::Swap, Instant::now());
        assert!(talk.swapped());
        assert_eq!(talk.slide(), 1);
        talk.apply(Action::Swap, Instant::now());
        assert!(!talk.swapped());
    }

    #[test]
    fn the_first_slide_change_starts_the_timer() {
        let t0 = Instant::now();
        let mut talk = Talk::new(3, Settings::default());
        talk.apply(Action::Help, t0);
        assert!(talk.timer().unused());
        talk.apply(Action::Next, t0);
        assert!(!talk.timer().unused());
        talk.apply(Action::ResetTimer, t0);
        talk.apply(Action::Previous, t0);
        assert!(!talk.timer().unused());
    }

    #[test]
    fn the_overview_selects_without_moving_the_talk() {
        let now = Instant::now();
        let mut talk = Talk::new(9, Settings::default());
        assert_eq!(talk.overview_columns(), 3);
        talk.apply(Action::Next, now);
        talk.apply(Action::Overview, now);
        assert_eq!(talk.overview(), Some(1));
        for action in [Action::Down, Action::Down, Action::Down, Action::Next] {
            talk.apply(action, now);
        }
        assert_eq!(talk.overview(), Some(8));
        assert_eq!(talk.slide(), 1);
        assert_eq!(talk.audience_slide(), Some(1));
        talk.apply(Action::Confirm, now);
        assert_eq!(talk.slide(), 8);
        assert_eq!(talk.overview(), None);
    }

    #[test]
    fn escape_closes_the_overview_and_a_click_jumps() {
        let now = Instant::now();
        let mut talk = Talk::new(5, Settings::default());
        talk.apply(Action::Overview, now);
        talk.apply(Action::Next, now);
        talk.apply(Action::Cancel, now);
        assert_eq!((talk.overview(), talk.slide()), (None, 0));
        talk.apply(Action::Overview, now);
        talk.apply(Action::Pick(3), now);
        assert_eq!((talk.overview(), talk.slide()), (None, 3));
    }

    #[test]
    fn overview_columns_fit_the_deck() {
        let columns = |slides| Talk::new(slides, Settings::default()).overview_columns();
        assert_eq!(
            [columns(1), columns(4), columns(5), columns(10), columns(16)],
            [1, 2, 3, 4, 4]
        );
    }

    #[test]
    fn a_reload_keeps_the_position_inside_the_new_deck() {
        let now = Instant::now();
        let mut talk = Talk::new(9, Settings::default());
        talk.apply(Action::Last, now);
        talk.apply(Action::Freeze, now);
        talk.apply(Action::Overview, now);
        talk.reload(12);
        assert_eq!((talk.slide(), talk.audience_slide()), (8, Some(8)));
        talk.reload(4);
        assert_eq!(talk.slide(), 3);
        assert_eq!(talk.audience_slide(), Some(3));
        assert_eq!(talk.overview(), Some(3));
        talk.reload(0);
        assert_eq!((talk.slide(), talk.overview()), (0, None));
    }

    #[test]
    fn goto_jumps_to_the_typed_slide() {
        let mut talk = Talk::new(12, Settings::default());
        for action in [Action::GoTo, Action::Digit(1), Action::Digit(1)] {
            talk.apply(action, Instant::now());
        }
        assert_eq!(talk.goto(), Some(11));
        assert_eq!(talk.slide(), 0);
        talk.apply(Action::Confirm, Instant::now());
        assert_eq!(talk.slide(), 10);
        assert_eq!(talk.goto(), None);
    }

    #[test]
    fn goto_ignores_digits_past_the_deck() {
        let mut talk = Talk::new(12, Settings::default());
        for action in [Action::GoTo, Action::Digit(4), Action::Digit(5)] {
            talk.apply(action, Instant::now());
        }
        assert_eq!(talk.goto(), Some(4));
        talk.apply(Action::Backspace, Instant::now());
        assert_eq!(talk.goto(), Some(0));
        talk.apply(Action::Confirm, Instant::now());
        assert_eq!(talk.slide(), 0);
    }

    #[test]
    fn other_keys_cancel_goto() {
        let mut talk = Talk::new(12, Settings::default());
        for action in [Action::GoTo, Action::Digit(5), Action::Next] {
            talk.apply(action, Instant::now());
        }
        assert_eq!(talk.goto(), None);
        assert_eq!(talk.slide(), 1);
        talk.apply(Action::Digit(3), Instant::now());
        talk.apply(Action::Confirm, Instant::now());
        assert_eq!(talk.slide(), 1);
    }

    #[test]
    fn blank_hides_the_slide_from_the_audience() {
        let mut talk = Talk::new(3, Settings::default());
        assert_eq!(talk.audience_slide(), Some(0));
        talk.apply(Action::Blank, Instant::now());
        assert_eq!(talk.audience_slide(), None);
        talk.apply(Action::Next, Instant::now());
        talk.apply(Action::Blank, Instant::now());
        assert_eq!(talk.audience_slide(), Some(1));
    }

    #[test]
    fn freeze_holds_the_audience_until_released() {
        let mut talk = Talk::new(3, Settings::default());
        talk.apply(Action::Freeze, Instant::now());
        talk.apply(Action::Next, Instant::now());
        talk.apply(Action::Next, Instant::now());
        assert_eq!(talk.slide(), 2);
        assert_eq!(talk.audience_slide(), Some(0));
        talk.apply(Action::Freeze, Instant::now());
        assert_eq!(talk.audience_slide(), Some(2));
    }

    #[test]
    fn the_laser_toggles_and_stays_on_across_slides() {
        let mut talk = Talk::new(3, Settings::default());
        assert!(!talk.laser());
        talk.apply(Action::Laser, Instant::now());
        talk.apply(Action::Next, Instant::now());
        talk.apply(Action::Cancel, Instant::now());
        assert!(talk.laser());
        talk.apply(Action::Laser, Instant::now());
        assert!(!talk.laser());
    }

    #[test]
    fn an_empty_deck_shows_the_audience_nothing() {
        assert_eq!(Talk::new(0, Settings::default()).audience_slide(), None);
    }

    #[test]
    fn layout_toggles_and_notes_resize() {
        let mut talk = Talk::new(3, Settings::default());
        talk.apply(Action::Layout, Instant::now());
        assert_eq!(talk.settings().layout, crate::settings::Layout::NotesFirst);
        let scale = talk.settings().notes_scale;
        talk.apply(Action::BiggerNotes, Instant::now());
        assert!(talk.settings().notes_scale > scale);
        talk.apply(Action::SmallerNotes, Instant::now());
        assert!((talk.settings().notes_scale - scale).abs() < 1e-4);
    }

    #[test]
    fn the_hint_toggles_and_escape_closes_it() {
        let mut talk = Talk::new(3, Settings::default());
        talk.apply(Action::Help, Instant::now());
        assert!(talk.help());
        talk.apply(Action::Help, Instant::now());
        assert!(!talk.help());
        talk.apply(Action::Help, Instant::now());
        talk.apply(Action::Cancel, Instant::now());
        assert!(!talk.help());
    }

    #[test]
    fn quit_needs_two_requests_in_a_row() {
        let mut talk = Talk::new(3, Settings::default());
        assert_eq!(talk.apply(Action::Quit, Instant::now()), Outcome::Continue);
        assert!(talk.quit_armed());
        assert_eq!(talk.apply(Action::Next, Instant::now()), Outcome::Continue);
        assert!(!talk.quit_armed());
        assert_eq!(talk.apply(Action::Quit, Instant::now()), Outcome::Continue);
        assert_eq!(talk.apply(Action::Quit, Instant::now()), Outcome::Quit);
    }
}
