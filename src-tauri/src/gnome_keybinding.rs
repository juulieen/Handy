//! GNOME Desktop keybinding integration for Linux/Wayland support.
//!
//! On GNOME with Wayland, Tauri's global shortcuts may not work reliably.
//! This module provides a fallback by setting up GNOME custom keybindings
//! that trigger Handy via the SIGUSR2 signal mechanism.

use log::{debug, error, info};
use std::process::Command;

/// The base path for GNOME custom keybindings in dconf/gsettings
const KEYBINDING_BASE_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";

/// The schema for custom keybindings
const KEYBINDING_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";

/// The schema for the list of custom keybindings
const MEDIA_KEYS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";

/// Check if we're running on a GNOME desktop environment
pub fn is_gnome_desktop() -> bool {
    // Check XDG_CURRENT_DESKTOP
    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        let desktop_lower = desktop.to_lowercase();
        if desktop_lower.contains("gnome") || desktop_lower.contains("unity") {
            return true;
        }
    }

    // Check DESKTOP_SESSION as fallback
    if let Ok(session) = std::env::var("DESKTOP_SESSION") {
        let session_lower = session.to_lowercase();
        if session_lower.contains("gnome") || session_lower.contains("ubuntu") {
            return true;
        }
    }

    // Check if gsettings is available and GNOME schemas exist
    is_gsettings_available()
}

/// Check if gsettings command is available and GNOME schemas are present
fn is_gsettings_available() -> bool {
    Command::new("gsettings")
        .args(["list-schemas"])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(MEDIA_KEYS_SCHEMA)
        })
        .unwrap_or(false)
}

/// Convert a Handy shortcut string (e.g., "ctrl+shift+s") to GNOME format (e.g., "<Control><Shift>s")
pub fn handy_shortcut_to_gnome(shortcut: &str) -> String {
    let mut result = String::new();
    let mut main_key = String::new();

    for part in shortcut.split('+') {
        let part_lower = part.trim().to_lowercase();
        match part_lower.as_str() {
            "ctrl" | "control" => result.push_str("<Control>"),
            "shift" => result.push_str("<Shift>"),
            "alt" | "option" => result.push_str("<Alt>"),
            "super" | "meta" | "win" | "windows" | "cmd" | "command" => result.push_str("<Super>"),
            _ => main_key = part.trim().to_string(),
        }
    }

    result.push_str(&main_key);
    result
}

/// Find the custom keybinding slot used by Handy, if any
fn find_handy_keybinding_slot() -> Option<String> {
    // Get the list of custom keybindings
    let output = Command::new("gsettings")
        .args(["get", MEDIA_KEYS_SCHEMA, "custom-keybindings"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let list_str = String::from_utf8_lossy(&output.stdout);

    // Parse the list (format: ['/path/custom0/', '/path/custom1/'])
    for slot in parse_keybinding_list(&list_str) {
        // Check if this slot is for Handy
        let name_output = Command::new("gsettings")
            .args([
                "get",
                &format!("{}:{}", KEYBINDING_SCHEMA, slot),
                "name",
            ])
            .output()
            .ok()?;

        if name_output.status.success() {
            let name = String::from_utf8_lossy(&name_output.stdout)
                .trim()
                .trim_matches('\'')
                .to_lowercase();
            if name == "handy" {
                return Some(slot);
            }
        }
    }

    None
}

/// Parse the GNOME keybinding list string into individual paths
fn parse_keybinding_list(list_str: &str) -> Vec<String> {
    let trimmed = list_str.trim();

    // Handle empty list
    if trimmed == "@as []" || trimmed == "[]" {
        return Vec::new();
    }

    // Parse the array format: ['/path1/', '/path2/']
    let inner = trimmed.trim_start_matches('[').trim_end_matches(']');

    inner
        .split(',')
        .filter_map(|s| {
            let cleaned = s.trim().trim_matches('\'').trim_matches('"');
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned.to_string())
            }
        })
        .collect()
}

/// Find the next available custom keybinding slot
fn find_next_available_slot() -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", MEDIA_KEYS_SCHEMA, "custom-keybindings"])
        .output()
        .ok()?;

    let list_str = String::from_utf8_lossy(&output.stdout);
    let existing = parse_keybinding_list(&list_str);

    // Find the highest existing slot number
    let mut max_num = -1i32;
    for slot in &existing {
        if let Some(num_str) = slot.split("custom").last() {
            if let Ok(num) = num_str.trim_end_matches('/').parse::<i32>() {
                max_num = max_num.max(num);
            }
        }
    }

    Some(format!("{}/custom{}/", KEYBINDING_BASE_PATH, max_num + 1))
}

/// Set up a GNOME custom keybinding for Handy
///
/// This creates or updates a custom keybinding that runs `pkill -USR2 -n handy`
/// when the specified shortcut is pressed.
pub fn setup_gnome_keybinding(shortcut: &str) -> Result<(), String> {
    if !is_gnome_desktop() {
        debug!("Not running on GNOME desktop, skipping GNOME keybinding setup");
        return Ok(());
    }

    let gnome_shortcut = handy_shortcut_to_gnome(shortcut);
    info!(
        "Setting up GNOME keybinding: {} -> {}",
        shortcut, gnome_shortcut
    );

    // Check if we already have a Handy keybinding
    let slot = find_handy_keybinding_slot().or_else(find_next_available_slot);

    let slot = match slot {
        Some(s) => s,
        None => {
            return Err("Could not determine keybinding slot".to_string());
        }
    };

    let schema_path = format!("{}:{}", KEYBINDING_SCHEMA, slot);

    // Set the keybinding properties
    let commands = [
        ("name", "handy"),
        ("command", "pkill -USR2 -n handy"),
        ("binding", &gnome_shortcut),
    ];

    for (key, value) in &commands {
        let output = Command::new("gsettings")
            .args(["set", &schema_path, key, value])
            .output()
            .map_err(|e| format!("Failed to run gsettings: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("gsettings set {} {} failed: {}", key, value, stderr);
            return Err(format!("Failed to set {}: {}", key, stderr));
        }
    }

    // Make sure this slot is in the custom-keybindings list
    add_slot_to_keybinding_list(&slot)?;

    info!(
        "Successfully set up GNOME keybinding at {} with shortcut {}",
        slot, gnome_shortcut
    );

    Ok(())
}

/// Add a slot to the custom-keybindings list if not already present
fn add_slot_to_keybinding_list(slot: &str) -> Result<(), String> {
    let output = Command::new("gsettings")
        .args(["get", MEDIA_KEYS_SCHEMA, "custom-keybindings"])
        .output()
        .map_err(|e| format!("Failed to get keybinding list: {}", e))?;

    let list_str = String::from_utf8_lossy(&output.stdout);
    let mut existing = parse_keybinding_list(&list_str);

    // Check if slot already exists
    if existing.iter().any(|s| s == slot) {
        return Ok(());
    }

    // Add the new slot
    existing.push(slot.to_string());

    // Format as GNOME array
    let new_list = format!(
        "[{}]",
        existing
            .iter()
            .map(|s| format!("'{}'", s))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let output = Command::new("gsettings")
        .args(["set", MEDIA_KEYS_SCHEMA, "custom-keybindings", &new_list])
        .output()
        .map_err(|e| format!("Failed to update keybinding list: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to update keybinding list: {}", stderr));
    }

    Ok(())
}

/// Remove the Handy GNOME keybinding
pub fn remove_gnome_keybinding() -> Result<(), String> {
    if !is_gnome_desktop() {
        return Ok(());
    }

    let slot = match find_handy_keybinding_slot() {
        Some(s) => s,
        None => {
            debug!("No Handy keybinding found to remove");
            return Ok(());
        }
    };

    info!("Removing GNOME keybinding at {}", slot);

    let schema_path = format!("{}:{}", KEYBINDING_SCHEMA, slot);

    // Clear the keybinding
    for key in &["name", "command", "binding"] {
        let _ = Command::new("gsettings")
            .args(["reset", &schema_path, key])
            .output();
    }

    // Remove from the list
    remove_slot_from_keybinding_list(&slot)?;

    Ok(())
}

/// Remove a slot from the custom-keybindings list
fn remove_slot_from_keybinding_list(slot: &str) -> Result<(), String> {
    let output = Command::new("gsettings")
        .args(["get", MEDIA_KEYS_SCHEMA, "custom-keybindings"])
        .output()
        .map_err(|e| format!("Failed to get keybinding list: {}", e))?;

    let list_str = String::from_utf8_lossy(&output.stdout);
    let existing: Vec<String> = parse_keybinding_list(&list_str)
        .into_iter()
        .filter(|s| s != slot)
        .collect();

    let new_list = if existing.is_empty() {
        "@as []".to_string()
    } else {
        format!(
            "[{}]",
            existing
                .iter()
                .map(|s| format!("'{}'", s))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    let output = Command::new("gsettings")
        .args(["set", MEDIA_KEYS_SCHEMA, "custom-keybindings", &new_list])
        .output()
        .map_err(|e| format!("Failed to update keybinding list: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to update keybinding list: {}", stderr));
    }

    Ok(())
}

/// Check if a GNOME keybinding already exists for Handy
pub fn has_gnome_keybinding() -> bool {
    if !is_gnome_desktop() {
        return false;
    }
    find_handy_keybinding_slot().is_some()
}

/// Get the current GNOME keybinding for Handy, if any
pub fn get_gnome_keybinding() -> Option<String> {
    if !is_gnome_desktop() {
        return None;
    }

    let slot = find_handy_keybinding_slot()?;
    let schema_path = format!("{}:{}", KEYBINDING_SCHEMA, slot);

    let output = Command::new("gsettings")
        .args(["get", &schema_path, "binding"])
        .output()
        .ok()?;

    if output.status.success() {
        let binding = String::from_utf8_lossy(&output.stdout)
            .trim()
            .trim_matches('\'')
            .to_string();
        if !binding.is_empty() {
            return Some(binding);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shortcut_conversion() {
        assert_eq!(handy_shortcut_to_gnome("ctrl+shift+s"), "<Control><Shift>s");
        assert_eq!(handy_shortcut_to_gnome("alt+space"), "<Alt>space");
        assert_eq!(handy_shortcut_to_gnome("super+h"), "<Super>h");
        assert_eq!(
            handy_shortcut_to_gnome("ctrl+alt+shift+f5"),
            "<Control><Alt><Shift>f5"
        );
    }

    #[test]
    fn test_parse_keybinding_list() {
        let list = "['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/']";
        let parsed = parse_keybinding_list(list);
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].contains("custom0"));

        let empty = "@as []";
        assert!(parse_keybinding_list(empty).is_empty());

        let multiple = "['/path/custom0/', '/path/custom1/']";
        let parsed = parse_keybinding_list(multiple);
        assert_eq!(parsed.len(), 2);
    }
}
