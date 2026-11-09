//! The state of a running talk, free of any GUI.

use crate::settings::Settings;

/// Something the speaker asks for, from a key or a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Go to the next slide.
    Next,
    /// Go to the previous slide.
    Previous,
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
    /// The slide number typed so far after `g`, 0 before the first digit.
    goto: Option<usize>,
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
            goto: None,
            settings,
            help: false,
            quit_armed: false,
        }
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

    /// Applies an action and says whether the app keeps running.
    pub fn apply(&mut self, action: Action) -> Outcome {
        if action == Action::Quit {
            if self.quit_armed {
                return Outcome::Quit;
            }
            self.quit_armed = true;
            return Outcome::Continue;
        }
        self.quit_armed = false;
        let last = self.slides.saturating_sub(1);
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
            Action::Next => self.slide = (self.slide + 1).min(last),
            Action::Previous => self.slide = self.slide.saturating_sub(1),
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
            Action::Layout => self.settings.layout = self.settings.layout.other(),
            Action::BiggerNotes => self.settings.bigger_notes(),
            Action::SmallerNotes => self.settings.smaller_notes(),
            Action::Help => self.help = !self.help,
            Action::Cancel => self.help = false,
            // The window that got the key toggles its own fullscreen.
            Action::Fullscreen => {}
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
        talk.apply(Action::Previous);
        assert_eq!(talk.slide(), 0);
        talk.apply(Action::Next);
        talk.apply(Action::Next);
        talk.apply(Action::Next);
        assert_eq!(talk.slide(), 2);
        assert_eq!(talk.next_slide(), None);
        talk.apply(Action::First);
        assert_eq!(talk.slide(), 0);
        assert_eq!(talk.next_slide(), Some(1));
        talk.apply(Action::Last);
        assert_eq!(talk.slide(), 2);
    }

    #[test]
    fn an_empty_deck_has_no_next_slide() {
        let mut talk = Talk::new(0, Settings::default());
        talk.apply(Action::Next);
        assert_eq!(talk.slide(), 0);
        assert_eq!(talk.next_slide(), None);
    }

    #[test]
    fn swap_toggles_and_keeps_the_slide() {
        let mut talk = Talk::new(3, Settings::default());
        talk.apply(Action::Next);
        talk.apply(Action::Swap);
        assert!(talk.swapped());
        assert_eq!(talk.slide(), 1);
        talk.apply(Action::Swap);
        assert!(!talk.swapped());
    }

    #[test]
    fn goto_jumps_to_the_typed_slide() {
        let mut talk = Talk::new(12, Settings::default());
        for action in [Action::GoTo, Action::Digit(1), Action::Digit(1)] {
            talk.apply(action);
        }
        assert_eq!(talk.goto(), Some(11));
        assert_eq!(talk.slide(), 0);
        talk.apply(Action::Confirm);
        assert_eq!(talk.slide(), 10);
        assert_eq!(talk.goto(), None);
    }

    #[test]
    fn goto_ignores_digits_past_the_deck() {
        let mut talk = Talk::new(12, Settings::default());
        for action in [Action::GoTo, Action::Digit(4), Action::Digit(5)] {
            talk.apply(action);
        }
        assert_eq!(talk.goto(), Some(4));
        talk.apply(Action::Backspace);
        assert_eq!(talk.goto(), Some(0));
        talk.apply(Action::Confirm);
        assert_eq!(talk.slide(), 0);
    }

    #[test]
    fn other_keys_cancel_goto() {
        let mut talk = Talk::new(12, Settings::default());
        for action in [Action::GoTo, Action::Digit(5), Action::Next] {
            talk.apply(action);
        }
        assert_eq!(talk.goto(), None);
        assert_eq!(talk.slide(), 1);
        talk.apply(Action::Digit(3));
        talk.apply(Action::Confirm);
        assert_eq!(talk.slide(), 1);
    }

    #[test]
    fn blank_hides_the_slide_from_the_audience() {
        let mut talk = Talk::new(3, Settings::default());
        assert_eq!(talk.audience_slide(), Some(0));
        talk.apply(Action::Blank);
        assert_eq!(talk.audience_slide(), None);
        talk.apply(Action::Next);
        talk.apply(Action::Blank);
        assert_eq!(talk.audience_slide(), Some(1));
    }

    #[test]
    fn freeze_holds_the_audience_until_released() {
        let mut talk = Talk::new(3, Settings::default());
        talk.apply(Action::Freeze);
        talk.apply(Action::Next);
        talk.apply(Action::Next);
        assert_eq!(talk.slide(), 2);
        assert_eq!(talk.audience_slide(), Some(0));
        talk.apply(Action::Freeze);
        assert_eq!(talk.audience_slide(), Some(2));
    }

    #[test]
    fn an_empty_deck_shows_the_audience_nothing() {
        assert_eq!(Talk::new(0, Settings::default()).audience_slide(), None);
    }

    #[test]
    fn layout_toggles_and_notes_resize() {
        let mut talk = Talk::new(3, Settings::default());
        talk.apply(Action::Layout);
        assert_eq!(talk.settings().layout, crate::settings::Layout::NotesFirst);
        let scale = talk.settings().notes_scale;
        talk.apply(Action::BiggerNotes);
        assert!(talk.settings().notes_scale > scale);
        talk.apply(Action::SmallerNotes);
        assert!((talk.settings().notes_scale - scale).abs() < 1e-4);
    }

    #[test]
    fn the_hint_toggles_and_escape_closes_it() {
        let mut talk = Talk::new(3, Settings::default());
        talk.apply(Action::Help);
        assert!(talk.help());
        talk.apply(Action::Help);
        assert!(!talk.help());
        talk.apply(Action::Help);
        talk.apply(Action::Cancel);
        assert!(!talk.help());
    }

    #[test]
    fn quit_needs_two_requests_in_a_row() {
        let mut talk = Talk::new(3, Settings::default());
        assert_eq!(talk.apply(Action::Quit), Outcome::Continue);
        assert!(talk.quit_armed());
        assert_eq!(talk.apply(Action::Next), Outcome::Continue);
        assert!(!talk.quit_armed());
        assert_eq!(talk.apply(Action::Quit), Outcome::Continue);
        assert_eq!(talk.apply(Action::Quit), Outcome::Quit);
    }
}
