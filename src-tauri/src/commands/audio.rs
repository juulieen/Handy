use crate::audio_feedback;
use crate::audio_toolkit::audio::{detect_best_microphone, list_input_devices, list_output_devices};
use crate::managers::audio::{AudioRecordingManager, MicrophoneMode};
use crate::settings::{get_settings, write_settings};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Type)]
pub struct CustomSounds {
    start: bool,
    stop: bool,
}

fn custom_sound_exists(app: &AppHandle, sound_type: &str) -> bool {
    app.path()
        .resolve(
            format!("custom_{}.wav", sound_type),
            tauri::path::BaseDirectory::AppData,
        )
        .map_or(false, |path| path.exists())
}

#[tauri::command]
#[specta::specta]
pub fn check_custom_sounds(app: AppHandle) -> CustomSounds {
    CustomSounds {
        start: custom_sound_exists(&app, "start"),
        stop: custom_sound_exists(&app, "stop"),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct AudioDevice {
    pub index: String,
    pub name: String,
    pub is_default: bool,
}

#[tauri::command]
#[specta::specta]
pub fn update_microphone_mode(app: AppHandle, always_on: bool) -> Result<(), String> {
    // Update settings
    let mut settings = get_settings(&app);
    settings.always_on_microphone = always_on;
    write_settings(&app, settings);

    // Update the audio manager mode
    let rm = app.state::<Arc<AudioRecordingManager>>();
    let new_mode = if always_on {
        MicrophoneMode::AlwaysOn
    } else {
        MicrophoneMode::OnDemand
    };

    rm.update_mode(new_mode)
        .map_err(|e| format!("Failed to update microphone mode: {}", e))
}

#[tauri::command]
#[specta::specta]
pub fn get_microphone_mode(app: AppHandle) -> Result<bool, String> {
    let settings = get_settings(&app);
    Ok(settings.always_on_microphone)
}

#[tauri::command]
#[specta::specta]
pub fn get_available_microphones() -> Result<Vec<AudioDevice>, String> {
    let devices =
        list_input_devices().map_err(|e| format!("Failed to list audio devices: {}", e))?;

    let mut result = vec![AudioDevice {
        index: "default".to_string(),
        name: "Default".to_string(),
        is_default: true,
    }];

    result.extend(devices.into_iter().map(|d| AudioDevice {
        index: d.index,
        name: d.name,
        is_default: false, // The explicit default is handled separately
    }));

    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub fn set_selected_microphone(app: AppHandle, device_name: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.selected_microphone = if device_name == "default" {
        None
    } else {
        Some(device_name)
    };
    write_settings(&app, settings);

    // Update the audio manager to use the new device
    let rm = app.state::<Arc<AudioRecordingManager>>();
    rm.update_selected_device()
        .map_err(|e| format!("Failed to update selected device: {}", e))?;

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_selected_microphone(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    Ok(settings
        .selected_microphone
        .unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
#[specta::specta]
pub fn get_available_output_devices() -> Result<Vec<AudioDevice>, String> {
    let devices =
        list_output_devices().map_err(|e| format!("Failed to list output devices: {}", e))?;

    let mut result = vec![AudioDevice {
        index: "default".to_string(),
        name: "Default".to_string(),
        is_default: true,
    }];

    result.extend(devices.into_iter().map(|d| AudioDevice {
        index: d.index,
        name: d.name,
        is_default: false, // The explicit default is handled separately
    }));

    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub fn set_selected_output_device(app: AppHandle, device_name: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.selected_output_device = if device_name == "default" {
        None
    } else {
        Some(device_name)
    };
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_selected_output_device(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    Ok(settings
        .selected_output_device
        .unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn play_test_sound(app: AppHandle, sound_type: String) {
    let sound = match sound_type.as_str() {
        "start" => audio_feedback::SoundType::Start,
        "stop" => audio_feedback::SoundType::Stop,
        _ => {
            warn!("Unknown sound type: {}", sound_type);
            return;
        }
    };
    audio_feedback::play_test_sound(&app, sound);
}

#[tauri::command]
#[specta::specta]
pub fn set_clamshell_microphone(app: AppHandle, device_name: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.clamshell_microphone = if device_name == "default" {
        None
    } else {
        Some(device_name)
    };
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_clamshell_microphone(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    Ok(settings
        .clamshell_microphone
        .unwrap_or_else(|| "default".to_string()))
}

/// Result of microphone auto-detection
#[derive(Serialize, Type)]
pub struct MicrophoneDetectionResult {
    /// The name of the detected microphone, if any
    pub detected_microphone: Option<String>,
    /// The amplitude level of the detected microphone
    pub amplitude: f32,
    /// Whether the detected microphone was automatically set as selected
    pub auto_selected: bool,
    /// Human-readable message about the detection result
    pub message: String,
}

/// Detect the best available microphone by testing audio levels
/// This briefly records from each microphone to measure ambient noise/audio levels
#[tauri::command]
#[specta::specta]
pub async fn auto_detect_microphone(app: AppHandle) -> Result<MicrophoneDetectionResult, String> {
    info!("Starting microphone auto-detection from UI");

    // Check if we're on PipeWire (Linux only)
    #[cfg(target_os = "linux")]
    let is_pipewire = crate::audio_toolkit::pipewire::is_pipewire_available();
    #[cfg(not(target_os = "linux"))]
    let is_pipewire = false;

    // Run the detection in a blocking task since it involves audio I/O
    let result = tokio::task::spawn_blocking(|| detect_best_microphone())
        .await
        .map_err(|e| format!("Detection task failed: {}", e))?
        .map_err(|e| format!("Detection failed: {}", e))?;

    match result {
        Some(best) => {
            let mic_name = best.name.clone();
            let amplitude = best.max_amplitude;

            // On PipeWire, the device selection is done at the system level via wpctl
            // So we don't need to update the app's selected_microphone setting
            // On other systems, we update the app settings
            if !is_pipewire {
                // Auto-select the detected microphone in app settings
                let mut settings = get_settings(&app);
                settings.selected_microphone = Some(mic_name.clone());
                write_settings(&app, settings);

                // Update the audio manager to use the new device
                let rm = app.state::<Arc<AudioRecordingManager>>();
                if let Err(e) = rm.update_selected_device() {
                    warn!("Failed to update audio manager with detected device: {}", e);
                }
            } else {
                info!("PipeWire: System default source updated via wpctl, app settings unchanged");
            }

            info!(
                "Auto-detected microphone: {} (amplitude: {:.6}){}",
                mic_name, amplitude,
                if is_pipewire { " [PipeWire]" } else { "" }
            );

            let message = if is_pipewire {
                format!(
                    "Set system default to '{}' (signal: {:.1}%)",
                    mic_name,
                    amplitude * 100.0
                )
            } else {
                format!(
                    "Detected and selected '{}' (signal: {:.1}%)",
                    mic_name,
                    amplitude * 100.0
                )
            };

            Ok(MicrophoneDetectionResult {
                detected_microphone: Some(mic_name),
                amplitude,
                auto_selected: true,
                message,
            })
        }
        None => {
            warn!("No suitable microphone detected");
            Ok(MicrophoneDetectionResult {
                detected_microphone: None,
                amplitude: 0.0,
                auto_selected: false,
                message: "No microphone with sufficient audio level detected. Please check your microphone connections and try again.".to_string(),
            })
        }
    }
}
