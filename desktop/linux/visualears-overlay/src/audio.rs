//! Cross-platform audio capture via cpal (ALSA/Pulse/PipeWire on Linux;
//! WASAPI/CoreAudio/AAudio elsewhere). On Windows, opening the default render endpoint as an input
//! stream activates cpal's native WASAPI loopback mode. Samples are downmixed to mono and linearly
//! resampled to 16 kHz — the same resample the macOS app used (verified fine for ASR; the aliasing
//! worry there turned out to be a red herring).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(not(target_os = "windows"))]
use std::io::Read;
use std::process::Child;
#[cfg(not(target_os = "windows"))]
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[cfg(not(target_os = "windows"))]
use std::thread;

pub struct Capture {
    _stream: Option<cpal::Stream>,
    _child: Option<Child>,
    pub source_rate: u32,
}

pub fn start(
    tx: Sender<Vec<f32>>,
    device_name: Option<&str>,
) -> Result<Capture, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let system_audio = is_system_audio_request(device_name);

    #[cfg(not(target_os = "windows"))]
    if system_audio {
        return start_pipewire_monitor(tx);
    }

    let select_input_device = || -> Result<_, Box<dyn std::error::Error>> {
        Ok(match device_name {
            Some("mic") => host
                .default_input_device()
                .ok_or("no default microphone/input device")?,
            Some(want) => {
                let want_l = want.to_lowercase();
                host.input_devices()?
                    .find(|d| {
                        d.name()
                            .map(|n| n.to_lowercase().contains(&want_l))
                            .unwrap_or(false)
                    })
                    .ok_or_else(|| {
                        format!("no input device matching '{want}' (try --list-audio)")
                    })?
            }
            None => host
                .default_input_device()
                .ok_or("no default input device")?,
        })
    };

    #[cfg(target_os = "windows")]
    let (device, cfg) = if system_audio {
        // CPAL's WASAPI backend treats an input stream built on an eRender endpoint as a native
        // AUDCLNT_STREAMFLAGS_LOOPBACK capture. Use the render endpoint's mix format, not an input
        // config (which output devices intentionally do not expose).
        let device = host
            .default_output_device()
            .ok_or("no default Windows output device available for WASAPI loopback")?;
        let cfg = device.default_output_config()?;
        eprintln!(
            "[audio] capturing system output via WASAPI loopback: {}",
            device.name().unwrap_or_else(|_| "?".into())
        );
        (device, cfg)
    } else {
        let device = select_input_device()?;
        let cfg = device.default_input_config()?;
        eprintln!(
            "[audio] capturing from: {}",
            device.name().unwrap_or_else(|_| "?".into())
        );
        (device, cfg)
    };

    #[cfg(not(target_os = "windows"))]
    let (device, cfg) = {
        let device = select_input_device()?;
        let cfg = device.default_input_config()?;
        eprintln!(
            "[audio] capturing from: {}",
            device.name().unwrap_or_else(|_| "?".into())
        );
        (device, cfg)
    };

    let src_rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;
    let fmt = cfg.sample_format();
    let stream_cfg: cpal::StreamConfig = cfg.into();
    let err_fn = |e| eprintln!("cpal stream error: {e}");

    let signal_reported = Arc::new(AtomicBool::new(false));
    let feed = move |mono: Vec<f32>| {
        report_signal_once(&mono, &signal_reported);
        let _ = tx.send(resample_linear(&mono, src_rate as f32, 16_000.0));
    };

    let stream = match fmt {
        cpal::SampleFormat::F32 => {
            let feed = feed.clone_box();
            device.build_input_stream(
                &stream_cfg,
                move |d: &[f32], _| feed(to_mono(d, channels)),
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let feed = feed.clone_box();
            device.build_input_stream(
                &stream_cfg,
                move |d: &[i16], _| {
                    let f: Vec<f32> = d.iter().map(|&s| s as f32 / 32768.0).collect();
                    feed(to_mono(&f, channels))
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::U16 => {
            let feed = feed.clone_box();
            device.build_input_stream(
                &stream_cfg,
                move |d: &[u16], _| {
                    let f: Vec<f32> = d.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).collect();
                    feed(to_mono(&f, channels))
                },
                err_fn,
                None,
            )?
        }
        other => return Err(format!("unsupported sample format {other:?}").into()),
    };
    stream.play()?;
    Ok(Capture {
        _stream: Some(stream),
        _child: None,
        source_rate: src_rate,
    })
}

fn is_system_audio_request(device_name: Option<&str>) -> bool {
    matches!(device_name, Some("system"))
}

#[cfg(not(target_os = "windows"))]
fn start_pipewire_monitor(tx: Sender<Vec<f32>>) -> Result<Capture, Box<dyn std::error::Error>> {
    let target = pipewire_monitor_target().unwrap_or_else(|| "0".to_string());
    eprintln!("[audio] capturing system sound via PipeWire monitor: {target}");
    let mut child = Command::new("pw-record")
        .args([
            "--target",
            &target,
            "--raw",
            "--rate",
            "16000",
            "--channels",
            "1",
            "--format",
            "f32",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().ok_or("pw-record stdout unavailable")?;
    thread::spawn(move || {
        let signal_reported = AtomicBool::new(false);
        let mut bytes = vec![0u8; 4096 * 4];
        loop {
            let Ok(n) = stdout.read(&mut bytes) else {
                break;
            };
            if n == 0 {
                break;
            }
            let samples: Vec<f32> = bytes[..n - (n % 4)]
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();
            report_signal_once(&samples, &signal_reported);
            if !samples.is_empty() && tx.send(samples).is_err() {
                break;
            }
        }
    });
    Ok(Capture {
        _stream: None,
        _child: Some(child),
        source_rate: 16_000,
    })
}

fn report_signal_once(samples: &[f32], reported: &AtomicBool) {
    if reported.load(Ordering::Relaxed) || samples.is_empty() {
        return;
    }
    let level =
        (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt();
    if level >= 0.0005 && !reported.swap(true, Ordering::Relaxed) {
        eprintln!("[audio] signal detected rms={level:.5}");
    }
}

#[cfg(not(target_os = "windows"))]
fn pipewire_monitor_target() -> Option<String> {
    // Resolve the concrete Audio/Sink node from PipeWire's own graph. In a Flatpak sandbox,
    // WirePlumber can resolve @DEFAULT_AUDIO_SINK@ to the microphone source; selecting the sink's
    // numeric node ID makes pw-record connect to monitor_FL/FR unambiguously.
    let output = Command::new("pw-dump").output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_pipewire_sink_id(&output.stdout).or_else(|| Some("@DEFAULT_AUDIO_SINK@".to_string()))
}

#[cfg(not(target_os = "windows"))]
fn parse_pipewire_sink_id(output: &[u8]) -> Option<String> {
    let graph: serde_json::Value = serde_json::from_slice(output).ok()?;
    graph
        .as_array()?
        .iter()
        .filter_map(|object| {
            let props = object.get("info")?.get("props")?;
            if props.get("media.class")?.as_str()? != "Audio/Sink" {
                return None;
            }
            let priority = props
                .get("priority.session")
                .and_then(|value| {
                    value
                        .as_str()
                        .and_then(|s| s.parse::<i64>().ok())
                        .or_else(|| value.as_i64())
                })
                .unwrap_or(0);
            Some((priority, object.get("id")?.as_u64()?))
        })
        .max_by_key(|(priority, _)| *priority)
        .map(|(_, id)| id.to_string())
}

pub fn list_input_devices() -> Result<(), Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    eprintln!("available input devices:");
    for d in host.input_devices()? {
        eprintln!("  - {}", d.name().unwrap_or_else(|_| "?".into()));
    }
    if let Some(def) = host.default_input_device() {
        eprintln!(
            "(system default input: {})",
            def.name().unwrap_or_else(|_| "?".into())
        );
    }
    #[cfg(target_os = "macos")]
    eprintln!("\nfor SYSTEM audio: install BlackHole (`brew install blackhole-2ch`), route output to it,\nthen run with:  --device \"BlackHole\"");
    #[cfg(target_os = "windows")]
    {
        if let Some(def) = host.default_output_device() {
            eprintln!(
                "(WASAPI system-loopback output: {})",
                def.name().unwrap_or_else(|_| "?".into())
            );
        }
        eprintln!("\nfor SYSTEM audio: use --device \"system\" to capture the default output through native WASAPI loopback.\nStereo Mix or VB-CABLE remain available as named input-device fallbacks.");
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    eprintln!("\nfor SYSTEM audio: expose a monitor/loopback source through PulseAudio/PipeWire,\nthen run with:  --device \"monitor\"");
    Ok(())
}

fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn resample_linear(input: &[f32], from: f32, to: f32) -> Vec<f32> {
    if (from - to).abs() < 1.0 || input.len() < 2 {
        return input.to_vec();
    }
    let out_len = ((input.len() as f32) * to / from).round().max(1.0) as usize;
    let ratio = (input.len() - 1) as f32 / (out_len.max(2) - 1) as f32;
    (0..out_len)
        .map(|i| {
            let x = i as f32 * ratio;
            let j = x.floor() as usize;
            let frac = x - j as f32;
            input[j] * (1.0 - frac) + input[(j + 1).min(input.len() - 1)] * frac
        })
        .collect()
}

// Tiny helper so the three format arms can share the boxed feed closure.
trait CloneBox {
    fn clone_box(&self) -> Box<dyn Fn(Vec<f32>) + Send>;
}
impl<F: Fn(Vec<f32>) + Send + Clone + 'static> CloneBox for F {
    fn clone_box(&self) -> Box<dyn Fn(Vec<f32>) + Send> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::is_system_audio_request;

    #[test]
    fn recognizes_the_system_audio_alias_without_stealing_named_devices() {
        assert!(is_system_audio_request(Some("system")));
        assert!(!is_system_audio_request(Some("mic")));
        assert!(!is_system_audio_request(Some("Stereo Mix")));
        assert!(!is_system_audio_request(None));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn selects_highest_priority_audio_sink_not_source() {
        use super::parse_pipewire_sink_id;

        let graph = br#"[
          {"id":61,"info":{"props":{"media.class":"Audio/Source","priority.session":"2000"}}},
          {"id":59,"info":{"props":{"media.class":"Audio/Sink","priority.session":"900"}}},
          {"id":60,"info":{"props":{"media.class":"Audio/Sink","priority.session":"1009"}}}
        ]"#;
        assert_eq!(parse_pipewire_sink_id(graph), Some("60".into()));
    }
}
