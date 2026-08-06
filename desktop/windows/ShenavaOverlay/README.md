# Shenava for Windows

Native Windows setup installers and portable packages for Shenava.

> **Release status (August 5, 2026): live captions restored.** A contributor
> verified Persian captions from a physical Realtek microphone and the default
> Windows System Audio loopback. The exact x64 release package was also
> installed on the project VM and produced Persian partial and final captions
> from the bundled deterministic speech fixture. The cloud VM exposes no
> microphone endpoint, so the physical-microphone result remains contributor
> hardware evidence rather than a simulated VM test.

This build intentionally ships one Persian path:

- Koochik HD streaming CTC model
- 3,669-word Static guide
- Rust-native tract runtime plus Rust CTC beam final pass

There is no model selector and no Vosk/ORT dependency in this package. Launch the app to open the same four-tab control surface as the macOS reference: Live, Display, Appearance, and About.
The installer always ships the pinned, checksum-verified `koochik_hd.onnx` model. Shenava warms that model while the control panel opens, then reuses the same resident worker across Start/Stop actions.

The app uses the shared Rust control panel plus the same native overlay surface as the macOS app: a transparent, always-on-top Persian caption strip positioned near the bottom center of the screen.

## Native Windows VM Development

The recommended Windows workflow builds with Microsoft's native MSVC tools and creates three standard Inno Setup executables. It does not install or invoke `cargo-xwin`.

Use a 64-bit Windows 10/11 VM. Clone this repository, open **PowerShell as Administrator** at the repository root, and run the idempotent bootstrap:

The exact failing Koochik runtime model is stored with Git LFS. Immediately
after cloning, make sure the real model—not an LFS pointer—has been downloaded:

```powershell
git lfs install
git lfs pull
```

The packaging script verifies the model's byte length and SHA-256 before it
creates an installer, and fails with a direct `git lfs pull` instruction if the
asset is missing or incomplete.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\desktop\windows\ShenavaOverlay\bootstrap-windows.ps1
```

The bootstrap installs missing prerequisites with `winget` and safely skips tools already present:

- Git for Windows
- Rustup plus the stable x64-hosted MSVC toolchain, the `x86_64`, `aarch64`, and `i686` Windows targets, `rustfmt`, and Clippy
- Visual Studio 2022 Build Tools with the x64/x86 and ARM64 C++ toolchains plus the recommended Windows SDK
- Inno Setup 6.3 or newer (`JRSoftware.InnoSetup`) for compiling the setup executables
- Microsoft Edge WebView2 Evergreen Runtime for running the control panel

If the VM image does not include `winget`, install or update **App Installer** first. The bootstrap can be rerun after a reboot or interrupted installation. Use `-SkipWebView2` only for a build-only machine that will not launch the app. Use `-SkipInstallerTools` only when producing portable packages rather than setup executables.

Then open a normal PowerShell at the repository root and build all three native payloads and setup executables:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\desktop\windows\ShenavaOverlay\build-installers.ps1
```

The pipeline builds each target sequentially, checks the packaged executable's PE machine field, and only then compiles its architecture-gated installer:

| Setup file | Rust payload target | Accepted Windows systems |
| --- | --- | --- |
| `Shenava-Setup-x64.exe` | `x86_64-pc-windows-msvc` | x64 Windows 10/11 |
| `Shenava-Setup-arm64.exe` | `aarch64-pc-windows-msvc` | ARM64 Windows 10/11 |
| `Shenava-Setup-x86.exe` | `i686-pc-windows-msvc` | 32-bit x86 Windows 10 (Pentium 4-class CPU or newer) |

The final artifacts are written to:

```text
.build\installers\Shenava-Setup-x64.exe
.build\installers\Shenava-Setup-arm64.exe
.build\installers\Shenava-Setup-x86.exe
.build\installers\SHA256SUMS.txt
```

The x86 build targets current 32-bit Windows, not Windows XP/Vista/7. Its Pentium 4-class CPU floor is a compatibility baseline, not a promise of real-time ASR speed on hardware that old. Each setup is deliberately rejected on the other two OS architectures so the three downloads cannot be confused.

Each setup checks Microsoft's documented WebView2 Runtime registry entry before it writes application files. If the Runtime is missing, Setup downloads Microsoft's approximately 2 MB Evergreen bootstrapper, which selects the device architecture and installs the current Runtime silently. Internet access is only required for that prerequisite step; if it fails, Setup stops with the official manual download URL and the underlying error or exit code instead of installing an app that cannot open its control panel.

To build just one setup executable:

```powershell
.\desktop\windows\ShenavaOverlay\build-installers.ps1 -Architecture x64
```

To recompile setup executables from existing architecture-specific portable packages without rebuilding Rust:

```powershell
.\desktop\windows\ShenavaOverlay\build-installers.ps1 -SkipBuild
```

For the original x64 development workflow, `build-native.ps1` writes `.build\Shenava-Windows` and its ZIP when called without arguments:

```powershell
.\desktop\windows\ShenavaOverlay\build-native.ps1
```

Choose another payload architecture or request a faster development compile without assembling a portable package:

```powershell
.\desktop\windows\ShenavaOverlay\build-native.ps1 -Architecture arm64
.\desktop\windows\ShenavaOverlay\build-native.ps1 -Architecture x86 -Configuration Debug -SkipPackage
```

To choose another repository-relative package path:

```powershell
.\desktop\windows\ShenavaOverlay\build-native.ps1 -OutDir ".build\Shenava-Windows-test"
```

After a packaged build, launch and smoke-test it:

```powershell
cd .build\Shenava-Windows
.\run.ps1 -RenderSmoke
.\run.ps1 -ReplayFixture
.\run.ps1
```

`-RenderSmoke` proves only Persian shaping and PNG rendering. `-ReplayFixture`
proves the deterministic fixture path. Neither is evidence that live
Microphone or System Audio captions work; complete the live acceptance matrix
in [CONTRIBUTING.md](CONTRIBUTING.md).

## Cross-build From macOS

From the repository root:

```powershell
pwsh desktop/windows/ShenavaOverlay/package.ps1
```

The existing package script uses the MSVC target via `cargo xwin` so the WebView2 control panel and tract native kernels link correctly from macOS. This remains separate from the native Windows workflow above.

The package is written to:

```text
.build/Shenava-Windows
```

## Install Or Run On Windows

Double-click the setup executable matching the machine. It installs Shenava under Program Files, creates a Start menu shortcut, offers an optional desktop shortcut, and registers a normal Windows uninstaller. An installer copied to the wrong architecture exits before writing any files. On a Windows 10 machine without WebView2, leave the computer online while Setup installs that prerequisite from Microsoft.

For a portable package, open PowerShell in the package directory and run:

```powershell
.\run.ps1
```

That opens the Shenava control panel. Use **شروع زیرنویس** to start captions, tune Display/Appearance, or show a demo caption.

The control panel uses Microsoft Edge WebView2. Current Windows 10/11 installs usually already have it; if the panel does not open, install the WebView2 Runtime from Microsoft and rerun `run.ps1`.

If Windows blocks PowerShell scripts, double-click `Shenava.cmd` instead, or run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\run.ps1
```

To list capture devices:

```powershell
.\run.ps1 -ListAudio
```

or double-click `ListAudioDevices.cmd`.

If Windows does not expose system audio loopback, enable a loopback-capable input such as Stereo Mix or a virtual audio device, then select it from the control panel.

## Smoke Tests

Render the shaped caption overlay without starting audio capture:

```powershell
.\run.ps1 -RenderSmoke
```

Run a bundled fixture through the full live overlay window:

```powershell
.\run.ps1 -ReplayFixture
```

or double-click `ReplayFixture.cmd`.
