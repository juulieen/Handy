use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SizedSample};
use log::{debug, info, warn};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct CpalDeviceInfo {
    pub index: String,
    pub name: String,
    pub is_default: bool,
    pub device: cpal::Device,
}

/// Result of testing a microphone's audio level
#[derive(Debug, Clone)]
pub struct MicrophoneTestResult {
    pub name: String,
    pub max_amplitude: f32,
    pub sample_count: usize,
}

/// Minimum amplitude threshold to consider a microphone as "working"
/// Normal speech typically has amplitude > 0.01, but ambient noise should be > 0.005
const MIN_WORKING_AMPLITUDE: f32 = 0.005;

/// Test duration for microphone detection (in milliseconds)
const TEST_DURATION_MS: u64 = 500;

/// Test a single microphone by briefly recording and measuring the audio level
pub fn test_microphone(device: &cpal::Device) -> Result<MicrophoneTestResult, Box<dyn std::error::Error + Send + Sync>> {
    let name = device.name().unwrap_or_else(|_| "Unknown".into());
    debug!("Testing microphone: {}", name);

    let config = device.default_input_config()?;
    let channels = config.channels() as usize;

    let max_amplitude = Arc::new(Mutex::new(0.0f32));
    let sample_count = Arc::new(Mutex::new(0usize));
    let max_amp_clone = max_amplitude.clone();
    let sample_count_clone = sample_count.clone();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build_test_stream::<f32>(device, &config, max_amp_clone, sample_count_clone, channels)?,
        cpal::SampleFormat::I16 => build_test_stream::<i16>(device, &config, max_amp_clone, sample_count_clone, channels)?,
        cpal::SampleFormat::I32 => build_test_stream::<i32>(device, &config, max_amp_clone, sample_count_clone, channels)?,
        cpal::SampleFormat::U8 => build_test_stream::<u8>(device, &config, max_amp_clone, sample_count_clone, channels)?,
        _ => return Err("Unsupported sample format".into()),
    };

    stream.play()?;
    std::thread::sleep(Duration::from_millis(TEST_DURATION_MS));
    drop(stream);

    let final_max = *max_amplitude.lock().unwrap();
    let final_count = *sample_count.lock().unwrap();

    debug!(
        "Microphone '{}': max_amplitude={:.6}, sample_count={}",
        name, final_max, final_count
    );

    Ok(MicrophoneTestResult {
        name,
        max_amplitude: final_max,
        sample_count: final_count,
    })
}

fn build_test_stream<T>(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    max_amplitude: Arc<Mutex<f32>>,
    sample_count: Arc<Mutex<usize>>,
    channels: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: Sample + SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    device.build_input_stream(
        &config.clone().into(),
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut max = max_amplitude.lock().unwrap();
            let mut count = sample_count.lock().unwrap();

            for chunk in data.chunks(channels) {
                // Average across channels for mono
                let mono_sample: f32 = chunk
                    .iter()
                    .map(|&s| s.to_sample::<f32>())
                    .sum::<f32>()
                    / channels as f32;

                let abs_val = mono_sample.abs();
                if abs_val > *max {
                    *max = abs_val;
                }
                *count += 1;
            }
        },
        |err| warn!("Stream error during microphone test: {}", err),
        None,
    )
}

/// Test all available microphones and return the one with the highest audio level
/// Returns None if no microphones are detected or all have very low levels
pub fn detect_best_microphone() -> Result<Option<MicrophoneTestResult>, Box<dyn std::error::Error + Send + Sync>> {
    info!("Starting microphone auto-detection...");

    // On Linux with PipeWire, use PipeWire-aware detection
    #[cfg(target_os = "linux")]
    {
        if crate::audio_toolkit::pipewire::is_pipewire_available() {
            info!("PipeWire detected, using PipeWire-aware detection");
            return detect_best_microphone_pipewire();
        }
    }

    // Fallback to standard cpal detection (works on macOS, Windows, and non-PipeWire Linux)
    detect_best_microphone_cpal()
}

/// PipeWire-aware microphone detection for Linux
/// This function enumerates PipeWire sources and tests each one
#[cfg(target_os = "linux")]
fn detect_best_microphone_pipewire() -> Result<Option<MicrophoneTestResult>, Box<dyn std::error::Error + Send + Sync>> {
    use crate::audio_toolkit::pipewire::{list_pipewire_sources, set_default_source, PipeWireSource};

    let sources = list_pipewire_sources()
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;

    if sources.is_empty() {
        warn!("No PipeWire audio sources found");
        return Ok(None);
    }

    info!("Found {} PipeWire audio sources", sources.len());

    let mut results: Vec<(PipeWireSource, MicrophoneTestResult)> = Vec::new();

    // Get the cpal device (should be "pipewire" on PipeWire systems)
    let devices = list_input_devices()?;
    let pipewire_device = devices.into_iter().next();

    if pipewire_device.is_none() {
        warn!("No cpal input device found");
        return Ok(None);
    }

    let device = pipewire_device.unwrap();

    for source in &sources {
        info!("Testing PipeWire source: {} (id: {})", source.name, source.id);

        // Set this source as the default
        if let Err(e) = set_default_source(source.id) {
            warn!("Failed to set source {} as default: {}", source.name, e);
            continue;
        }

        // Give PipeWire time to switch
        std::thread::sleep(Duration::from_millis(200));

        // Test the audio level
        match test_microphone(&device.device) {
            Ok(mut result) => {
                // Use the PipeWire source name instead of "pipewire"
                result.name = source.name.clone();
                info!(
                    "Source '{}': amplitude={:.6}, samples={}",
                    source.name, result.max_amplitude, result.sample_count
                );
                results.push((source.clone(), result));
            }
            Err(e) => {
                warn!("Failed to test source '{}': {}", source.name, e);
            }
        }

        // Small delay between tests
        std::thread::sleep(Duration::from_millis(100));
    }

    // Note: We don't restore the original default source here because
    // we want to keep the best source as the system default after detection

    if results.is_empty() {
        warn!("No PipeWire sources could be tested");
        return Ok(None);
    }

    // Sort by max_amplitude descending
    results.sort_by(|a, b| {
        b.1.max_amplitude
            .partial_cmp(&a.1.max_amplitude)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Log all results
    info!("PipeWire microphone detection results:");
    for (i, (source, result)) in results.iter().enumerate() {
        let status = if result.max_amplitude >= MIN_WORKING_AMPLITUDE {
            "GOOD"
        } else {
            "LOW"
        };
        info!(
            "  {}. {} (id:{}) - amplitude: {:.6} [{}]",
            i + 1,
            result.name,
            source.id,
            result.max_amplitude,
            status
        );
    }

    // Get the best result
    let best = results.into_iter().next();

    if let Some((best_source, ref result)) = best {
        if result.max_amplitude >= MIN_WORKING_AMPLITUDE {
            // Set the best source as the new default
            info!(
                "Setting best source as default: {} (id: {}, amplitude: {:.6})",
                result.name, best_source.id, result.max_amplitude
            );
            if let Err(e) = set_default_source(best_source.id) {
                warn!("Failed to set best source as default: {}", e);
            }
            return Ok(Some(result.clone()));
        } else {
            warn!(
                "Best source '{}' has low amplitude ({:.6}), below threshold ({:.6})",
                result.name, result.max_amplitude, MIN_WORKING_AMPLITUDE
            );
            // Still return the best result even if below threshold
            return Ok(Some(result.clone()));
        }
    }

    Ok(None)
}

/// Standard cpal-based microphone detection (for non-PipeWire systems)
fn detect_best_microphone_cpal() -> Result<Option<MicrophoneTestResult>, Box<dyn std::error::Error + Send + Sync>> {
    let devices = list_input_devices()?;

    if devices.is_empty() {
        warn!("No input devices found");
        return Ok(None);
    }

    let mut results: Vec<MicrophoneTestResult> = Vec::new();

    for device_info in devices {
        match test_microphone(&device_info.device) {
            Ok(result) => {
                results.push(result);
            }
            Err(e) => {
                warn!("Failed to test microphone '{}': {}", device_info.name, e);
            }
        }
        // Small delay between tests to let audio system settle
        std::thread::sleep(Duration::from_millis(100));
    }

    if results.is_empty() {
        warn!("No microphones could be tested");
        return Ok(None);
    }

    // Sort by max_amplitude descending
    results.sort_by(|a, b| b.max_amplitude.partial_cmp(&a.max_amplitude).unwrap_or(std::cmp::Ordering::Equal));

    // Log all results
    info!("Microphone detection results:");
    for (i, result) in results.iter().enumerate() {
        let status = if result.max_amplitude >= MIN_WORKING_AMPLITUDE {
            "GOOD"
        } else {
            "LOW"
        };
        info!(
            "  {}. {} - amplitude: {:.6} [{}]",
            i + 1,
            result.name,
            result.max_amplitude,
            status
        );
    }

    // Return the best one if it meets the minimum threshold
    let best = results.into_iter().next();
    if let Some(ref b) = best {
        if b.max_amplitude >= MIN_WORKING_AMPLITUDE {
            info!("Best microphone detected: {} (amplitude: {:.6})", b.name, b.max_amplitude);
            return Ok(Some(b.clone()));
        } else {
            warn!(
                "Best microphone '{}' has low amplitude ({:.6}), below threshold ({:.6})",
                b.name, b.max_amplitude, MIN_WORKING_AMPLITUDE
            );
        }
    }

    Ok(best)
}

pub fn list_input_devices() -> Result<Vec<CpalDeviceInfo>, Box<dyn std::error::Error + Send + Sync>> {
    let host = crate::audio_toolkit::get_cpal_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    let mut out = Vec::<CpalDeviceInfo>::new();

    for (index, device) in host.input_devices()?.enumerate() {
        let name = device.name().unwrap_or_else(|_| "Unknown".into());

        let is_default = Some(name.clone()) == default_name;

        out.push(CpalDeviceInfo {
            index: index.to_string(),
            name,
            is_default,
            device,
        });
    }

    Ok(out)
}

pub fn list_output_devices() -> Result<Vec<CpalDeviceInfo>, Box<dyn std::error::Error + Send + Sync>> {
    let host = crate::audio_toolkit::get_cpal_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());

    let mut out = Vec::<CpalDeviceInfo>::new();

    for (index, device) in host.output_devices()?.enumerate() {
        let name = device.name().unwrap_or_else(|_| "Unknown".into());

        let is_default = Some(name.clone()) == default_name;

        out.push(CpalDeviceInfo {
            index: index.to_string(),
            name,
            is_default,
            device,
        });
    }

    Ok(out)
}
