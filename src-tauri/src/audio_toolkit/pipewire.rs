/// PipeWire utilities for Linux audio device enumeration
/// This module provides functions to interact with PipeWire via wpctl

use log::{debug, info};
use std::process::Command;

/// Represents a PipeWire audio source
#[derive(Debug, Clone)]
pub struct PipeWireSource {
    /// The PipeWire node ID
    pub id: u32,
    /// The human-readable name
    pub name: String,
    /// Whether this is the current default source
    pub is_default: bool,
    /// The volume level (if available)
    pub volume: Option<f32>,
}

/// Check if PipeWire is available on the system
pub fn is_pipewire_available() -> bool {
    // Use "wpctl status" to check if wpctl is available and PipeWire is running
    // Note: "wpctl --version" doesn't exist, so we use "status" instead
    Command::new("wpctl")
        .arg("status")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// List available audio sources from PipeWire
pub fn list_pipewire_sources() -> Result<Vec<PipeWireSource>, String> {
    let output = Command::new("wpctl")
        .arg("status")
        .output()
        .map_err(|e| format!("Failed to run wpctl: {}", e))?;

    if !output.status.success() {
        return Err("wpctl status failed".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_wpctl_sources(&stdout)
}

/// Parse wpctl status output to extract audio sources
fn parse_wpctl_sources(output: &str) -> Result<Vec<PipeWireSource>, String> {
    let mut sources = Vec::new();
    let mut in_sources_section = false;

    for line in output.lines() {
        // Detect the Sources section under Audio
        if line.contains("Sources:") {
            in_sources_section = true;
            continue;
        }

        // Exit when we hit the next section
        if in_sources_section
            && (line.contains("Source endpoints:")
                || line.contains("Streams:")
                || (line.contains("├─") && !line.contains("Sources"))
                || (line.contains("└─") && !line.contains("Sources")))
        {
            // Check if this is a subsection header, not a source
            if !line.trim().starts_with(|c: char| c.is_ascii_digit()) && !line.contains("*") {
                in_sources_section = false;
                continue;
            }
        }

        if in_sources_section {
            // Parse lines like:
            // │      39. Raptor Lake-P/U/H cAVS Headphones Stereo Microphone [vol: 1.00]
            // │  *   59. Raptor Lake-P/U/H cAVS Digital Microphone [vol: 1.00]
            if let Some(source) = parse_source_line(line) {
                sources.push(source);
            }
        }
    }

    debug!("Parsed {} PipeWire sources", sources.len());
    Ok(sources)
}

/// Parse a single source line from wpctl output
fn parse_source_line(line: &str) -> Option<PipeWireSource> {
    // Remove tree characters and trim
    let cleaned = line
        .replace("│", "")
        .replace("├", "")
        .replace("└", "")
        .replace("─", "")
        .trim()
        .to_string();

    if cleaned.is_empty() {
        return None;
    }

    // Check if it's the default (marked with *)
    let is_default = cleaned.starts_with('*');
    let cleaned = cleaned.trim_start_matches('*').trim();

    // Parse the ID (number before the dot)
    let parts: Vec<&str> = cleaned.splitn(2, '.').collect();
    if parts.len() != 2 {
        return None;
    }

    let id: u32 = parts[0].trim().parse().ok()?;
    let rest = parts[1].trim();

    // Extract volume if present [vol: X.XX]
    let (name, volume) = if let Some(vol_start) = rest.find("[vol:") {
        let name = rest[..vol_start].trim().to_string();
        let vol_str = &rest[vol_start..];
        let volume = vol_str
            .trim_start_matches("[vol:")
            .trim_end_matches(']')
            .trim()
            .replace(" MUTED", "")
            .parse::<f32>()
            .ok();
        (name, volume)
    } else {
        (rest.to_string(), None)
    };

    if name.is_empty() {
        return None;
    }

    Some(PipeWireSource {
        id,
        name,
        is_default,
        volume,
    })
}

/// Set the default audio source in PipeWire
pub fn set_default_source(source_id: u32) -> Result<(), String> {
    info!("Setting PipeWire default source to ID {}", source_id);

    let output = Command::new("wpctl")
        .arg("set-default")
        .arg(source_id.to_string())
        .output()
        .map_err(|e| format!("Failed to run wpctl set-default: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("wpctl set-default failed: {}", stderr));
    }

    Ok(())
}

/// Test a PipeWire source by briefly setting it as default and recording
/// Returns the max amplitude detected
pub fn test_pipewire_source(
    source: &PipeWireSource,
    test_fn: impl FnOnce() -> Result<f32, String>,
) -> Result<f32, String> {
    debug!("Testing PipeWire source: {} (id: {})", source.name, source.id);

    // Temporarily set this source as default
    set_default_source(source.id)?;

    // Give PipeWire a moment to switch
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Run the test
    let result = test_fn();

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_source_line() {
        let line = "│      39. Raptor Lake-P/U/H cAVS Headphones Stereo Microphone [vol: 1.00]";
        let source = parse_source_line(line).unwrap();
        assert_eq!(source.id, 39);
        assert_eq!(
            source.name,
            "Raptor Lake-P/U/H cAVS Headphones Stereo Microphone"
        );
        assert!(!source.is_default);
        assert_eq!(source.volume, Some(1.0));
    }

    #[test]
    fn test_parse_default_source_line() {
        let line = "│  *   59. Raptor Lake-P/U/H cAVS Digital Microphone [vol: 1.00]";
        let source = parse_source_line(line).unwrap();
        assert_eq!(source.id, 59);
        assert_eq!(source.name, "Raptor Lake-P/U/H cAVS Digital Microphone");
        assert!(source.is_default);
        assert_eq!(source.volume, Some(1.0));
    }

    #[test]
    fn test_parse_muted_source() {
        let line = "│  *   40. Raptor Lake-P/U/H cAVS HDMI / DisplayPort 1 Output [vol: 0.40 MUTED]";
        let source = parse_source_line(line).unwrap();
        assert_eq!(source.id, 40);
        assert!(source.is_default);
        assert_eq!(source.volume, Some(0.40));
    }

    #[test]
    fn test_parse_wpctl_sources() {
        let output = r#"Audio
 ├─ Devices:
 │      51. PCM2912A Audio Codec                [alsa]
 │      52. Raptor Lake-P/U/H cAVS              [alsa]
 │
 ├─ Sinks:
 │      33. Some Sink [vol: 1.00]
 │
 ├─ Sources:
 │      39. Raptor Lake-P/U/H cAVS Headphones Stereo Microphone [vol: 1.00]
 │      58. PCM2912A Audio Codec Mono           [vol: 1.00]
 │  *   59. Raptor Lake-P/U/H cAVS Digital Microphone [vol: 1.00]
 │
 ├─ Source endpoints:
 │
 └─ Streams:"#;

        let sources = parse_wpctl_sources(output).unwrap();
        assert_eq!(sources.len(), 3);
        assert_eq!(sources[0].id, 39);
        assert_eq!(sources[1].id, 58);
        assert_eq!(sources[2].id, 59);
        assert!(sources[2].is_default);
    }
}
