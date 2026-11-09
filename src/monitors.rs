//! Choosing the monitor for the audience window.

use std::sync::{Arc, Mutex};

/// Monitor names in winit's `available_monitors()` order, shared by the event loop and the GUI.
pub type Monitors = Arc<Mutex<Vec<String>>>;

/// Connector prefixes of built-in laptop panels.
const INTERNAL: [&str; 3] = ["eDP-", "LVDS-", "DSI-"];

/// Picks the audience monitor among `names` and returns its index.
///
/// `requested` names a connector, such as `DP-6`. Without it, the first external monitor wins.
/// A single monitor never gets an audience window, so the speaker keeps the presenter view.
///
/// An empty name is a Wayland output whose name has not arrived yet. It neither counts nor wins.
pub fn audience_monitor(names: &[String], requested: Option<&str>) -> Option<usize> {
    if names.iter().filter(|n| !n.is_empty()).count() < 2 {
        return None;
    }
    match requested {
        Some(name) => names.iter().position(|n| n == name),
        None => names
            .iter()
            .position(|n| !n.is_empty() && !INTERNAL.iter().any(|prefix| n.starts_with(prefix))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn picks_the_first_external_monitor() {
        assert_eq!(audience_monitor(&names(&["eDP-1", "DP-6"]), None), Some(1));
        assert_eq!(
            audience_monitor(&names(&["DP-3", "eDP-1", "HDMI-A-1"]), None),
            Some(0)
        );
        assert_eq!(
            audience_monitor(&names(&["LVDS-1", "DSI-1", "HDMI-A-1"]), None),
            Some(2)
        );
    }

    #[test]
    fn internal_panels_alone_get_no_audience() {
        assert_eq!(audience_monitor(&names(&["eDP-1", "eDP-2"]), None), None);
    }

    #[test]
    fn one_monitor_gets_no_audience_even_when_requested() {
        assert_eq!(audience_monitor(&names(&["DP-6"]), None), None);
        assert_eq!(audience_monitor(&names(&["DP-6"]), Some("DP-6")), None);
        assert_eq!(audience_monitor(&[], None), None);
    }

    #[test]
    fn an_output_without_a_name_yet_is_skipped() {
        // A new Wayland output has no name until its name event arrives.
        assert_eq!(audience_monitor(&names(&["eDP-1", ""]), None), None);
        assert_eq!(
            audience_monitor(&names(&["eDP-1", "", "DP-6"]), None),
            Some(2)
        );
    }

    #[test]
    fn a_requested_connector_wins() {
        let list = names(&["eDP-1", "DP-6", "HDMI-A-1"]);
        assert_eq!(audience_monitor(&list, Some("HDMI-A-1")), Some(2));
        assert_eq!(audience_monitor(&list, Some("eDP-1")), Some(0));
    }

    #[test]
    fn a_missing_requested_connector_gets_no_audience() {
        assert_eq!(
            audience_monitor(&names(&["eDP-1", "DP-6"]), Some("DP-7")),
            None
        );
    }
}
