//! What window chrome this environment expects the app to draw.
//!
//! The window is undecorated (`decorations: false` in `tauri.conf.json`), so
//! the title bar is the app's own — which means the app has to answer a
//! question the toolkit used to answer for it: *which* controls belong on it
//! here.
//!
//! * macOS keeps its native traffic lights, so we draw none and leave room for
//!   them.
//! * A tiling compositor places and sizes windows itself. Minimize and
//!   maximize are not implemented there — `xdg_toplevel.set_minimized` is
//!   ignored, and "maximized" has no meaning in a tiled layout — so a button
//!   for either is a button that does nothing.
//! * Everywhere else: minimize, maximize and close.
//!
//! Wayland has an exact answer to this in `xdg_toplevel.wm_capabilities`, and
//! X11 in `_NET_WM_ALLOWED_ACTIONS`, but neither is surfaced by the window
//! layer underneath us. Reading the environment is the approximation, kept to
//! one table so it is cheap to correct.
//!
//! Kept in step with the same file in the sibling app (simple-sheet): both draw
//! their own title bar and face the same question.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowChrome {
    /// Whether the app draws window controls at all. False where the platform
    /// still draws its own.
    pub controls: bool,
    pub minimize: bool,
    pub maximize: bool,
    /// Keep the left of the title bar clear for the platform's own controls.
    pub traffic_light_inset: bool,
}

/// Close is always offered: every window manager can close a window.
const CLOSE_ONLY: WindowChrome = WindowChrome {
    controls: true,
    minimize: false,
    maximize: false,
    traffic_light_inset: false,
};

const EVERYTHING: WindowChrome = WindowChrome {
    controls: true,
    minimize: true,
    maximize: true,
    traffic_light_inset: false,
};

const NATIVE: WindowChrome = WindowChrome {
    controls: false,
    minimize: false,
    maximize: false,
    traffic_light_inset: true,
};

pub fn detect() -> WindowChrome {
    chrome_for(std::env::consts::OS, |key| std::env::var(key).ok())
}

fn chrome_for(os: &str, env: impl Fn(&str) -> Option<String>) -> WindowChrome {
    match os {
        "macos" => NATIVE,
        "windows" => EVERYTHING,
        _ if is_tiling(&env) => CLOSE_ONLY,
        _ => EVERYTHING,
    }
}

/// A compositor's own socket is the least ambiguous signal it is running:
/// `XDG_CURRENT_DESKTOP` is set by whoever launched the session and is often
/// something else entirely, or missing.
const TILING_SOCKETS: &[&str] = &[
    "SWAYSOCK",
    "I3SOCK",
    "HYPRLAND_INSTANCE_SIGNATURE",
    "NIRI_SOCKET",
];

/// Names as they appear in `XDG_CURRENT_DESKTOP` / `XDG_SESSION_DESKTOP`,
/// matched case-insensitively against each colon-separated entry.
const TILING_NAMES: &[&str] = &[
    "sway",
    "i3",
    "hyprland",
    "niri",
    "river",
    "dwl",
    "dwm",
    "awesome",
    "bspwm",
    "xmonad",
    "qtile",
    "herbstluftwm",
    "spectrwm",
    "leftwm",
];

fn is_tiling(env: &impl Fn(&str) -> Option<String>) -> bool {
    if TILING_SOCKETS.iter().any(|key| env(key).is_some()) {
        return true;
    }
    ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "DESKTOP_SESSION"]
        .iter()
        .filter_map(|key| env(key))
        .any(|value| {
            value
                .split(':')
                .any(|name| TILING_NAMES.iter().any(|t| name.trim().eq_ignore_ascii_case(t)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn macos_keeps_its_own_controls() {
        let chrome = chrome_for("macos", env_of(&[]));
        assert!(!chrome.controls, "the traffic lights are already there");
        assert!(chrome.traffic_light_inset);
    }

    #[test]
    fn windows_gets_the_full_set() {
        assert_eq!(chrome_for("windows", env_of(&[])), EVERYTHING);
    }

    #[test]
    fn a_tiling_compositor_gets_close_only() {
        // Whichever way it announces itself.
        for env in [
            vec![("SWAYSOCK", "/run/user/1000/sway-ipc.sock")],
            vec![("HYPRLAND_INSTANCE_SIGNATURE", "abc123")],
            vec![("NIRI_SOCKET", "/run/user/1000/niri.sock")],
            vec![("XDG_CURRENT_DESKTOP", "sway")],
            vec![("XDG_CURRENT_DESKTOP", "Hyprland")],
            vec![("XDG_SESSION_DESKTOP", "river")],
            vec![("DESKTOP_SESSION", "xmonad")],
        ] {
            assert_eq!(
                chrome_for("linux", env_of(&env)),
                CLOSE_ONLY,
                "expected a tiling layout for {env:?}"
            );
        }
    }

    #[test]
    fn a_desktop_that_stacks_windows_gets_the_full_set() {
        for env in [
            vec![("XDG_CURRENT_DESKTOP", "GNOME")],
            vec![("XDG_CURRENT_DESKTOP", "KDE")],
            vec![("XDG_CURRENT_DESKTOP", "XFCE")],
            // A session that says nothing at all: assume the usual buttons
            // work rather than hiding controls someone may need.
            vec![],
        ] {
            assert_eq!(
                chrome_for("linux", env_of(&env)),
                EVERYTHING,
                "expected the full set for {env:?}"
            );
        }
    }

    #[test]
    fn a_compound_desktop_string_is_matched_entry_by_entry() {
        // Wrapped sessions list several, e.g. `sway:wlroots`, and a name that
        // merely contains a tiler's name is not one: `i3` must not match
        // `i3-but-not-really`, nor `sway` match `swaybar`.
        assert_eq!(
            chrome_for("linux", env_of(&[("XDG_CURRENT_DESKTOP", "sway:wlroots")])),
            CLOSE_ONLY
        );
        assert_eq!(
            chrome_for("linux", env_of(&[("XDG_CURRENT_DESKTOP", "swaybar")])),
            EVERYTHING
        );
    }
}
