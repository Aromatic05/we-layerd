pub(crate) const BUS_NAME: &str = "io.github.weLayerd.Gnome";
pub(crate) const OBJECT_PATH: &str = "/io/github/weLayerd/Gnome";
pub(crate) const WINDOW_BRIDGE_INTERFACE: &str = "io.github.weLayerd.Gnome.WindowBridge";
#[allow(dead_code)]
pub(crate) const VIDEO_BRIDGE_INTERFACE: &str = "io.github.weLayerd.Gnome.VideoBridge";
pub(crate) const PING_METHOD: &str = "Ping";
pub(crate) const REGISTER_WINDOW_METHOD: &str = "RegisterWindow";
pub(crate) const UNREGISTER_WINDOW_METHOD: &str = "UnregisterWindow";
pub(crate) const GET_WINDOW_STATES_METHOD: &str = "GetWindowStates";
pub(crate) const REGISTER_OUTPUT_WINDOW_METHOD: &str = "RegisterOutputWindow";
pub(crate) const SHOW_BACKGROUND_MENU_METHOD: &str = "ShowBackgroundMenu";

/// First extension bridge revision that accepts one registered renderer window
/// per output and pins each window to its connector's monitor.
const FIRST_OUTPUT_WINDOW_BRIDGE_REVISION: u32 = 6;

/// Parses the `gnome-window-bridge-v<N>-...` revision reported by `Ping`.
pub(crate) fn supports_output_windows(ping_version: &str) -> bool {
    ping_version
        .strip_prefix("gnome-window-bridge-v")
        .and_then(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u32>().ok()
        })
        .is_some_and(|revision| revision >= FIRST_OUTPUT_WINDOW_BRIDGE_REVISION)
}

#[cfg(test)]
mod tests {
    use super::supports_output_windows;

    #[test]
    fn output_window_support_follows_bridge_revision() {
        assert!(!supports_output_windows("gnome-window-bridge-v5-monitor-connectors;metadata=4"));
        assert!(supports_output_windows("gnome-window-bridge-v6-output-windows;metadata=5"));
        assert!(supports_output_windows("gnome-window-bridge-v12-future"));
        assert!(!supports_output_windows("unrelated"));
    }
}
