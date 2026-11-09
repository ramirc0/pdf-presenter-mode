//! The state of a running talk, free of any GUI.

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
    /// Swap what the two windows show.
    Swap,
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
    help: bool,
    quit_armed: bool,
}

impl Talk {
    /// Starts a talk on the first of `slides` slides.
    pub fn new(slides: usize) -> Self {
        Self {
            slide: 0,
            slides,
            swapped: false,
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

    /// Whether the presenter window shows the audience view and the other way around.
    pub fn swapped(&self) -> bool {
        self.swapped
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
        match action {
            Action::Next => self.slide = (self.slide + 1).min(last),
            Action::Previous => self.slide = self.slide.saturating_sub(1),
            Action::First => self.slide = 0,
            Action::Last => self.slide = last,
            Action::Swap => self.swapped = !self.swapped,
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
        let mut talk = Talk::new(3);
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
        let mut talk = Talk::new(0);
        talk.apply(Action::Next);
        assert_eq!(talk.slide(), 0);
        assert_eq!(talk.next_slide(), None);
    }

    #[test]
    fn swap_toggles_and_keeps_the_slide() {
        let mut talk = Talk::new(3);
        talk.apply(Action::Next);
        talk.apply(Action::Swap);
        assert!(talk.swapped());
        assert_eq!(talk.slide(), 1);
        talk.apply(Action::Swap);
        assert!(!talk.swapped());
    }

    #[test]
    fn the_hint_toggles_and_escape_closes_it() {
        let mut talk = Talk::new(3);
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
        let mut talk = Talk::new(3);
        assert_eq!(talk.apply(Action::Quit), Outcome::Continue);
        assert!(talk.quit_armed());
        assert_eq!(talk.apply(Action::Next), Outcome::Continue);
        assert!(!talk.quit_armed());
        assert_eq!(talk.apply(Action::Quit), Outcome::Continue);
        assert_eq!(talk.apply(Action::Quit), Outcome::Quit);
    }
}
