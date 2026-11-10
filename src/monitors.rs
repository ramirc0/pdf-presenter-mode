//! Choosing the monitor for the audience window.

use std::sync::{Arc, Mutex};

use eframe::egui;

/// A connected monitor.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// The connector name, such as `DP-6`. Empty until a new Wayland output's name arrives.
    pub name: String,
    /// The width and height in physical pixels.
    pub pixels: [u32; 2],
    /// Where the compositor puts it, in logical pixels.
    /// Wayland gives winit only whole scales, so with fractional scaling it comes out too small.
    pub area: egui::Rect,
}

/// The connected monitors in winit's `available_monitors()` order, shared by the event loop and the GUI.
pub type Monitors = Arc<Mutex<Vec<Monitor>>>;

/// The names of `monitors`, in order.
pub fn names_of(monitors: &[Monitor]) -> Vec<String> {
    monitors
        .iter()
        .map(|monitor| monitor.name.clone())
        .collect()
}

/// Connector prefixes of built-in laptop panels.
const INTERNAL: [&str; 3] = ["eDP-", "LVDS-", "DSI-"];

/// Picks the audience monitor among `names` and returns its index.
///
/// `requested` names a connector, such as `DP-6`. Without it, the first external monitor wins,
/// unless it is `presenter`, the monitor the presenter is fullscreen on.
/// A single monitor never gets an audience window, so the speaker keeps the presenter view.
///
/// An empty name is a Wayland output whose name has not arrived yet. It neither counts nor wins.
pub fn audience_monitor(
    names: &[String],
    requested: Option<&str>,
    presenter: Option<&str>,
) -> Option<usize> {
    if names.iter().filter(|n| !n.is_empty()).count() < 2 {
        return None;
    }
    match requested {
        Some(name) => names.iter().position(|n| n == name),
        None => names.iter().position(|n| {
            !n.is_empty()
                && Some(n.as_str()) != presenter
                && !INTERNAL.iter().any(|prefix| n.starts_with(prefix))
        }),
    }
}

/// Picks the monitor after `current` among `names` for the audience, wrapping around and
/// skipping `presenter`, the presenter's monitor. Returns `None` if no other monitor can take it.
pub fn next_audience_monitor(
    names: &[String],
    current: Option<&str>,
    presenter: Option<&str>,
) -> Option<usize> {
    let now = current.and_then(|current| names.iter().position(|name| name == current));
    let first = now.map_or(0, |now| now + 1);
    (0..names.len())
        .map(|step| (first + step) % names.len())
        .find(|&index| {
            Some(index) != now
                && !names[index].is_empty()
                && Some(names[index].as_str()) != presenter
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audience_moves_on_past_the_presenter_and_around() {
        let list = names(&["eDP-1", "DP-1", "DP-2"]);
        let next = |current| next_audience_monitor(&list, current, Some("eDP-1"));
        assert_eq!(next(Some("DP-1")), Some(2));
        assert_eq!(next(Some("DP-2")), Some(1));
        assert_eq!(next(None), Some(1));
    }

    #[test]
    fn with_no_other_monitor_the_audience_stays() {
        let list = names(&["eDP-1", "DP-1"]);
        assert_eq!(
            next_audience_monitor(&list, Some("DP-1"), Some("eDP-1")),
            None
        );
        assert_eq!(next_audience_monitor(&[], None, None), None);
    }

    #[test]
    fn a_monitor_without_a_name_yet_is_skipped() {
        let list = names(&["eDP-1", "", "DP-2"]);
        assert_eq!(next_audience_monitor(&list, None, Some("eDP-1")), Some(2));
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn picks_the_first_external_monitor() {
        assert_eq!(
            audience_monitor(&names(&["eDP-1", "DP-6"]), None, None),
            Some(1)
        );
        assert_eq!(
            audience_monitor(&names(&["DP-3", "eDP-1", "HDMI-A-1"]), None, None),
            Some(0)
        );
        assert_eq!(
            audience_monitor(&names(&["LVDS-1", "DSI-1", "HDMI-A-1"]), None, None),
            Some(2)
        );
    }

    #[test]
    fn internal_panels_alone_get_no_audience() {
        assert_eq!(
            audience_monitor(&names(&["eDP-1", "eDP-2"]), None, None),
            None
        );
    }

    #[test]
    fn one_monitor_gets_no_audience_even_when_requested() {
        assert_eq!(audience_monitor(&names(&["DP-6"]), None, None), None);
        assert_eq!(
            audience_monitor(&names(&["DP-6"]), Some("DP-6"), None),
            None
        );
        assert_eq!(audience_monitor(&[], None, None), None);
    }

    #[test]
    fn an_output_without_a_name_yet_is_skipped() {
        // A new Wayland output has no name until its name event arrives.
        assert_eq!(audience_monitor(&names(&["eDP-1", ""]), None, None), None);
        assert_eq!(
            audience_monitor(&names(&["eDP-1", "", "DP-6"]), None, None),
            Some(2)
        );
    }

    #[test]
    fn a_requested_connector_wins() {
        let list = names(&["eDP-1", "DP-6", "HDMI-A-1"]);
        assert_eq!(audience_monitor(&list, Some("HDMI-A-1"), None), Some(2));
        assert_eq!(audience_monitor(&list, Some("eDP-1"), None), Some(0));
    }

    #[test]
    fn the_auto_pick_skips_the_presenters_monitor() {
        let list = names(&["DP-1", "DP-2"]);
        assert_eq!(audience_monitor(&list, None, Some("DP-1")), Some(1));
        assert_eq!(audience_monitor(&list, None, Some("DP-2")), Some(0));
        assert_eq!(
            audience_monitor(&names(&["eDP-1", "DP-1"]), None, Some("DP-1")),
            None
        );
    }

    #[test]
    fn a_missing_requested_connector_gets_no_audience() {
        assert_eq!(
            audience_monitor(&names(&["eDP-1", "DP-6"]), Some("DP-7"), None),
            None
        );
    }
}
