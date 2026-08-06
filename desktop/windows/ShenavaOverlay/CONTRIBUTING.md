# Contributing to Shenava for Windows

Thank you for helping keep live Persian captions reliable on Windows. Preserve
the evidence matrix below when changing capture, model loading, decoding, or the
native overlay, and submit the smallest evidence-backed fix.

## Verification status

As of August 5, 2026:

- the original no-caption report was traced to a silent, roughly 103–112 second
  ONNX warm-up on the contributor's Windows machine;
- the control panel now reports model progress and keeps one warm caption worker
  alive across Start/Stop actions;
- a physical Realtek microphone produced RMS activity and visible Persian
  captions on contributor hardware;
- default Windows System Audio loopback produced visible Persian captions on
  contributor hardware;
- the exact packaged x64 release installs as Shenava and emits Persian partial
  and final captions from its bundled speech fixture;
- the cloud VM has no microphone input device, so it cannot serve as proof that
  microphone capture works on a normal Windows machine.

Do not treat a window opening, a render smoke image, or an audio-signal log alone
as live-caption proof. A real spoken phrase must appear in the overlay.

## Fresh-clone setup

Use 64-bit Windows 10 or 11 and PowerShell:

```powershell
git clone https://github.com/Reza2kn/VisualEars.git
cd VisualEars
git lfs install
git lfs pull
powershell -NoProfile -ExecutionPolicy Bypass -File .\desktop\windows\ShenavaOverlay\bootstrap-windows.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\desktop\windows\ShenavaOverlay\build-installers.ps1 -Architecture x64
```

For faster iteration, build an unpackaged debug executable:

```powershell
.\desktop\windows\ShenavaOverlay\build-native.ps1 -Architecture x64 -Configuration Debug -SkipPackage
```

## Reproduction matrix

Run these separately and record the result of each:

1. `RenderSmoke.cmd` — Persian shaping/rendering only.
2. `ReplayFixture.cmd` — packaged model, feature extraction, decoding, and
   overlay using the bundled WAV.
3. `ListAudioDevices.cmd` — exact input devices and system-loopback endpoint.
4. Microphone mode — speak a short Persian phrase into a physical microphone.
5. System Audio mode — play clear Persian speech through the default output.

For modes 4 and 5, confirm all of the following:

- the control panel reports that captions started;
- `%LOCALAPPDATA%\Shenava\launcher.log` names the selected capture path;
- the log contains `[audio] signal detected rms=...` while speech plays;
- at least one partial or final recognition event is produced;
- visible Persian text appears in the native overlay.

## Useful evidence for a pull request

Attach or paste:

- Windows version and CPU architecture;
- build type and commit SHA;
- the output of `ListAudioDevices.cmd`;
- the relevant section of `%LOCALAPPDATA%\Shenava\launcher.log` with personal
  device names redacted if desired;
- whether `ReplayFixture.cmd` showed text;
- whether Microphone and System Audio each detected a signal;
- the exact stage where captions stopped.

Never commit recordings, credentials, machine addresses, WebView2 user data,
build output, or installer binaries to a pull request.

## Acceptance contract

A Windows live-caption fix is ready for review when a clean x64 checkout builds
and the same executable visibly captions both:

- a real microphone phrase; and
- spoken audio played through Windows system output.

Keep render smoke, fixture replay, microphone capture, and system loopback as
four separate results. ARM64 and x86 packaging must still compile, but native
live-audio verification on those architectures can be reported separately.
