//! OS light/dark appearance detection.
//!
//! There is no portable way to subscribe to appearance changes, so this polls
//! a platform probe on a background thread and reports transitions. The probes
//! are cheap (one short-lived process every few seconds) and only run when the
//! user turns following on.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Forces the answer where no probe can give one (bare window managers,
/// containers, remote sessions). `dark` / `light`, case-insensitive.
pub const APPEARANCE_ENV: &str = "GARGO_OS_APPEARANCE";

const POLL_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    pub fn is_dark(self) -> bool {
        self == Appearance::Dark
    }
}

/// Current OS appearance, or `None` when nothing can answer.
pub fn detect() -> Option<Appearance> {
    if let Some(forced) = appearance_from_env(std::env::var(APPEARANCE_ENV).ok().as_deref()) {
        return Some(forced);
    }
    probe()
}

pub fn appearance_from_env(value: Option<&str>) -> Option<Appearance> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "dark" => Some(Appearance::Dark),
        "light" => Some(Appearance::Light),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn probe() -> Option<Appearance> {
    // The key exists only while dark mode is on: a failed read *is* the
    // answer, not an error.
    let output = Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .ok()?;
    Some(appearance_from_macos_output(
        output.status.success(),
        &String::from_utf8_lossy(&output.stdout),
    ))
}

#[cfg(target_os = "linux")]
fn probe() -> Option<Appearance> {
    let color_scheme = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()?;
    if color_scheme.status.success()
        && let Some(appearance) =
            appearance_from_color_scheme(&String::from_utf8_lossy(&color_scheme.stdout))
    {
        return Some(appearance);
    }

    // `color-scheme = default` means "no preference expressed"; the theme name
    // is the only remaining hint.
    let gtk_theme = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
        .ok()?;
    if !gtk_theme.status.success() {
        return None;
    }
    appearance_from_gtk_theme(&String::from_utf8_lossy(&gtk_theme.stdout))
}

#[cfg(target_os = "windows")]
fn probe() -> Option<Appearance> {
    let output = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    appearance_from_reg_query(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn probe() -> Option<Appearance> {
    None
}

/// macOS: any successful read of `AppleInterfaceStyle` means dark.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn appearance_from_macos_output(success: bool, stdout: &str) -> Appearance {
    if success && stdout.trim().eq_ignore_ascii_case("dark") {
        Appearance::Dark
    } else {
        Appearance::Light
    }
}

/// GNOME: `'prefer-dark'` / `'prefer-light'` / `'default'`. `default` is not an
/// answer, so it returns `None` and the caller falls back to the theme name.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn appearance_from_color_scheme(stdout: &str) -> Option<Appearance> {
    let value = stdout.trim().trim_matches('\'').to_ascii_lowercase();
    match value.as_str() {
        "prefer-dark" => Some(Appearance::Dark),
        "prefer-light" => Some(Appearance::Light),
        _ => None,
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn appearance_from_gtk_theme(stdout: &str) -> Option<Appearance> {
    let value = stdout.trim().trim_matches('\'').to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    if value.ends_with("-dark") || value.contains("dark") {
        Some(Appearance::Dark)
    } else {
        Some(Appearance::Light)
    }
}

/// Windows: `AppsUseLightTheme REG_DWORD 0x0` is dark.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn appearance_from_reg_query(stdout: &str) -> Option<Appearance> {
    let token = stdout.split_whitespace().last()?;
    let value = token
        .strip_prefix("0x")
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .or_else(|| token.parse::<u32>().ok())?;
    Some(if value == 0 {
        Appearance::Dark
    } else {
        Appearance::Light
    })
}

pub struct AppearanceRuntimeHandle {
    pub event_rx: mpsc::Receiver<Appearance>,
    stop: Arc<AtomicBool>,
}

impl AppearanceRuntimeHandle {
    /// Start polling. Emits the current appearance once, then only on change.
    ///
    /// The worker is never joined: it can be up to one poll interval away from
    /// noticing the stop flag, and quitting the editor must not wait on that.
    /// The flag exists so a long-lived process that drops the handle stops
    /// spawning probes, not to make shutdown synchronous.
    pub fn new() -> Result<Self, String> {
        let (event_tx, event_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);

        thread::Builder::new()
            .name("gargo-appearance-runtime".to_string())
            .spawn(move || {
                let mut last: Option<Appearance> = None;
                while !worker_stop.load(Ordering::Relaxed) {
                    if let Some(current) = detect()
                        && last != Some(current)
                    {
                        last = Some(current);
                        if event_tx.send(current).is_err() {
                            return;
                        }
                    }
                    thread::sleep(POLL_INTERVAL);
                }
            })
            .map_err(|e| format!("failed to spawn appearance runtime worker: {}", e))?;

        Ok(Self { event_rx, stop })
    }
}

impl Drop for AppearanceRuntimeHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_wins_over_any_probe() {
        assert_eq!(appearance_from_env(Some("dark")), Some(Appearance::Dark));
        assert_eq!(
            appearance_from_env(Some(" LIGHT ")),
            Some(Appearance::Light)
        );
        assert_eq!(appearance_from_env(Some("purple")), None);
        assert_eq!(appearance_from_env(None), None);
    }

    #[test]
    fn macos_missing_key_means_light() {
        assert_eq!(
            appearance_from_macos_output(true, "Dark\n"),
            Appearance::Dark
        );
        // `defaults read` fails when the key is absent — that is light mode,
        // not a failure to detect.
        assert_eq!(appearance_from_macos_output(false, ""), Appearance::Light);
    }

    #[test]
    fn gnome_color_scheme_default_is_not_an_answer() {
        assert_eq!(
            appearance_from_color_scheme("'prefer-dark'\n"),
            Some(Appearance::Dark)
        );
        assert_eq!(
            appearance_from_color_scheme("'prefer-light'\n"),
            Some(Appearance::Light)
        );
        assert_eq!(appearance_from_color_scheme("'default'\n"), None);
    }

    #[test]
    fn gnome_falls_back_to_the_theme_name() {
        assert_eq!(
            appearance_from_gtk_theme("'Adwaita-dark'\n"),
            Some(Appearance::Dark)
        );
        assert_eq!(
            appearance_from_gtk_theme("'Adwaita'\n"),
            Some(Appearance::Light)
        );
        assert_eq!(appearance_from_gtk_theme("\n"), None);
    }

    #[test]
    fn windows_zero_means_dark() {
        let dark = "    AppsUseLightTheme    REG_DWORD    0x0\n";
        let light = "    AppsUseLightTheme    REG_DWORD    0x1\n";
        assert_eq!(appearance_from_reg_query(dark), Some(Appearance::Dark));
        assert_eq!(appearance_from_reg_query(light), Some(Appearance::Light));
        assert_eq!(appearance_from_reg_query(""), None);
    }

    #[test]
    fn dropping_the_handle_stops_the_worker() {
        let handle = AppearanceRuntimeHandle::new().expect("spawn");
        let stop = Arc::clone(&handle.stop);
        drop(handle);
        assert!(stop.load(Ordering::Relaxed));
    }
}
