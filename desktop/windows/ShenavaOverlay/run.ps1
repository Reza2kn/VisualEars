param(
  [string]$Device = "",
  [switch]$ListAudio,
  [switch]$ReplayFixture,
  [switch]$RenderSmoke,
  [switch]$English
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Exe = Join-Path $Root "Shenava.exe"
$FastModel = Join-Path $Root "Models\ShenavaStreaming\koochik_hd.nnef.tgz"
$OnnxModel = Join-Path $Root "Models\ShenavaStreaming\koochik_hd.onnx"
$EnglishModel = Join-Path $Root "Models\ShenavaStreaming\english.onnx"
$Model = if ($English) { $EnglishModel } elseif (Test-Path $FastModel) { $FastModel } else { $OnnxModel }
$ModelKey = if ($English) { "english" } else { "koochik_hd" }
$Tokens = if ($English) { Join-Path $Root "Models\ShenavaStreaming\english.tokens.txt" } else { Join-Path $Root "Models\ShenavaStreaming\tokens.txt" }
$Hotwords = if ($English) { Join-Path $Root "Models\ShenavaStreaming\hotwords_en.txt" } else { Join-Path $Root "Models\ShenavaStreaming\hotwords_fa.txt" }
$Mel = Join-Path $Root "engine\mel_filters_slaney_80x257.json"

if (!(Test-Path $Exe)) { throw "Missing executable: $Exe" }
if (!(Test-Path $Model)) { throw "Missing model: $Model" }
if (!(Test-Path $Tokens)) { throw "Missing tokens: $Tokens" }
if (!(Test-Path $Hotwords)) { throw "Missing Static-3669 word list: $Hotwords" }
if (!(Test-Path $Mel)) { throw "Missing mel filter bank: $Mel" }

if ($ListAudio) {
  & $Exe --list-audio
  exit $LASTEXITCODE
}

if ($RenderSmoke) {
  $Out = Join-Path $Root "caption-smoke.png"
  $SmokeText = -join ([char[]](0x632,0x6cc,0x631,0x646,0x648,0x6cc,0x633,0x20,0x634,0x646,0x648,0x627,0x20,0x628,0x631,0x627,0x6cc,0x20,0x648,0x6cc,0x646,0x62f,0x648,0x632))
  & $Exe --render-caption $SmokeText $Out
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Write-Host "Wrote $Out"
  exit 0
}

$CommonArgs = @($ModelKey, $Model, $Tokens, $Mel, "--hotwords", $Hotwords)

if ($ReplayFixture) {
  $Fixture = Join-Path $Root "fixtures\golha_clear.wav"
  if (!(Test-Path $Fixture)) { throw "Missing replay fixture: $Fixture" }
  & $Exe --overlay-wav $ModelKey $Model $Tokens $Mel $Fixture --hotwords $Hotwords
  exit $LASTEXITCODE
}

& $Exe --control @CommonArgs
exit $LASTEXITCODE
