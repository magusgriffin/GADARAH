# Post-install bootstrap for the Oracle's local LLM stack.
#
# Invoked by the GADARAH MSI when the user selects the "Install Ollama +
# DeepSeek R1 1.5B" optional feature. Safe to re-run: each step short-circuits
# when the target artifact already exists.
#
# The script does NOT download heavy models silently. It:
#   1. Downloads and runs the official Ollama Windows installer.
#   2. Waits for the Ollama service to come up.
#   3. Pulls deepseek-r1:1.5b (1.1 GB).
#
# Stdout is captured by the installer's deferred custom action.

$ErrorActionPreference = "Stop"

function Write-Step($msg) {
    Write-Host "[gadarah-bootstrap] $msg"
}

# ── 1. Ollama ───────────────────────────────────────────────────────────────
$ollamaExe = Join-Path $env:LOCALAPPDATA "Programs\Ollama\ollama.exe"
if (-not (Test-Path $ollamaExe)) {
    Write-Step "Ollama not found — downloading installer"
    $installer = Join-Path $env:TEMP "OllamaSetup.exe"
    Invoke-WebRequest -Uri "https://ollama.com/download/OllamaSetup.exe" -OutFile $installer
    Write-Step "Running Ollama installer (silent)"
    Start-Process -FilePath $installer -ArgumentList "/VERYSILENT" -Wait
} else {
    Write-Step "Ollama already present at $ollamaExe"
}

# ── 2. Wait for the service ─────────────────────────────────────────────────
Write-Step "Waiting for Ollama service on 127.0.0.1:11434"
$ready = $false
for ($i = 0; $i -lt 30; $i++) {
    try {
        $r = Invoke-WebRequest -Uri "http://127.0.0.1:11434/api/tags" -UseBasicParsing -TimeoutSec 2
        if ($r.StatusCode -eq 200) { $ready = $true; break }
    } catch { Start-Sleep -Seconds 2 }
}
if (-not $ready) {
    Write-Step "Ollama did not come up within 60 s — skipping model pull. User can pull it later from the Oracle tab."
    exit 0
}

# ── 3. Model pull ───────────────────────────────────────────────────────────
Write-Step "Pulling deepseek-r1:1.5b (~1.1 GB)"
& $ollamaExe pull deepseek-r1:1.5b
Write-Step "Bootstrap complete."
