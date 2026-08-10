# Luminus Pool installer (PRIVATE) for Windows (PowerShell 5.1+ / 7+).
#
# This is the private repo's installer. It mirrors install.ps1 in the public
# repo but defaults to the private repo URL. The private build includes extra
# providers (gitlab-duo, youmind) - installer flow is identical.
#
# One-command install (requires SSH key configured for the private repo):
#   irm https://raw.githubusercontent.com/priyo000/luminus/main/install.ps1 | iex
#
# Or after cloning:
#   powershell -ExecutionPolicy Bypass -File install.ps1
#
# Environment variables (all optional):
#   $env:LUMINUS_HOME          Install directory (default: $HOME\luminus-pool)
#   $env:LUMINUS_REPO          Repo URL (default: github.com/priyo000/luminus - PRIVATE)
#   $env:LUMINUS_YES = "1"     Skip confirmation (CI / unattended)
#   $env:LUMINUS_BRANCH        Branch to clone (default: main)
#   $env:LUMINUS_NO_CLI = "1"  Skip the luminus CLI in ~\.local\bin
#   $env:LUMINUS_SKIP_BROWSERS = "1"  Skip Playwright/Camoufox download

#Requires -Version 5.1

$ErrorActionPreference = "Stop"

$RepoUrl     = if ($env:LUMINUS_REPO)    { $env:LUMINUS_REPO }    else { "git@github.com:priyo000/luminus.git" }
$DefaultDir  = if ($env:LUMINUS_HOME)    { $env:LUMINUS_HOME }    else { Join-Path $HOME "luminus-pool" }
$Branch      = if ($env:LUMINUS_BRANCH)  { $env:LUMINUS_BRANCH }  else { "main" }
$AssumeYes   = $env:LUMINUS_YES -eq "1"

function Step([string]$msg) { Write-Host "==> " -ForegroundColor Cyan -NoNewline; Write-Host $msg -ForegroundColor White }
function Info([string]$msg) { Write-Host "    $msg" }
function Warn([string]$msg) { Write-Host "!!  $msg" -ForegroundColor Yellow }
function Fail([string]$msg) { Write-Host "xx  $msg" -ForegroundColor Red; exit 1 }
function Ok  ([string]$msg) { Write-Host "ok  " -ForegroundColor Green -NoNewline; Write-Host $msg }

function Have([string]$cmd) { return [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }

# Windows only runs files whose extension is in PATHEXT (.EXE/.CMD/...). An
# extensionless PE (e.g. Kiro-Cli's "bun" shim) is found by Get-Command but
# launches the "Select an app to open" dialog. Require a runnable file.
function Test-RunnableApp([string]$cmd) {
    $cmdInfo = Get-Command $cmd -ErrorAction SilentlyContinue
    if (-not $cmdInfo) { return $false }
    $path = $cmdInfo.Source
    if (-not $path -or -not (Test-Path -LiteralPath $path)) { return $false }
    $ext = [System.IO.Path]::GetExtension($path)
    if (-not $ext) { return $false }
    $pathext = ($env:PATHEXT -split ';') | ForEach-Object { $_.ToLowerInvariant() }
    if ($pathext -notcontains $ext.ToLowerInvariant()) { return $false }
    return $true
}

# Prefer a real bun.exe. Reject extensionless "bun" binaries that only work in Git Bash.
function Get-BunCommand {
    foreach ($cand in @("bun.exe", "bun")) {
        if (-not (Have $cand)) { continue }
        if (-not (Test-RunnableApp $cand)) {
            $src = (Get-Command $cand -ErrorAction SilentlyContinue).Source
            Warn "Found non-runnable '$cand' at $src (no .exe extension) - ignoring"
            continue
        }
        # Smoke-test: must print a version without throwing
        $prev = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            $ver = & $cand --version 2>&1 | Out-String
            if ($LASTEXITCODE -eq 0 -and $ver -match '\d+\.\d+') {
                return $cand
            }
        } catch {}
        finally { $ErrorActionPreference = $prev }
    }
    return $null
}

# Refresh PATH from registry - winget/scoop/choco may have updated it
function Refresh-Path {
    $machine = [System.Environment]::GetEnvironmentVariable("Path", "Machine")
    $user    = [System.Environment]::GetEnvironmentVariable("Path", "User")
    $env:Path = "$machine;$user"
}

function Add-PathOnce([string]$dir) {
    if (-not (Test-Path $dir)) { return }
    $current = $env:Path -split ';'
    if ($current -notcontains $dir) {
        $env:Path = "$dir;$env:Path"
    }
}

# Some Windows installs ship `python.exe` as a Microsoft Store stub that opens
# the Store and exits 0 with no real interpreter. Detect and reject it.
function Test-RealPython([string]$cmd) {
    # PS 5.1: native stderr becomes ErrorRecords; don't let Stop kill us
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $r = & $cmd --version 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) { return $false }
        if ($r -match "Python \d+\.\d+") { return $true }
        return $false
    } catch { return $false }
    finally { $ErrorActionPreference = $prev }
}

# Run a native (external) command. Windows PowerShell 5.1 turns native stderr
# into ErrorRecords; with $ErrorActionPreference=Stop those become terminating
# even when the process exits 0 (e.g. pip progress on stderr). Temporarily
# set Continue so only the exit code decides success/failure.
function Invoke-Native {
    param(
        [Parameter(Mandatory)] [scriptblock]$Action,
        [switch]$Quiet
    )
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        if ($Quiet) {
            $null = & $Action 2>&1
        } else {
            & $Action
        }
        if ($null -ne $LASTEXITCODE) { return $LASTEXITCODE }
        return 0
    } finally {
        $ErrorActionPreference = $prev
    }
}

# Retry a script block with exponential backoff for flaky network steps
function Retry-Action {
    param(
        [Parameter(Mandatory)] [scriptblock]$Action,
        [int]$MaxAttempts = 3,
        [int]$DelaySeconds = 3
    )
    $attempt = 0
    $delay = $DelaySeconds
    while ($true) {
        $attempt++
        $exit = 1
        $prev = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            & $Action
            $exit = if ($null -ne $LASTEXITCODE) { $LASTEXITCODE } else { 0 }
        } catch {
            $exit = 1
            if ($attempt -ge $MaxAttempts) {
                $ErrorActionPreference = $prev
                throw
            }
        } finally {
            $ErrorActionPreference = $prev
        }
        if ($exit -eq 0) { return }
        if ($attempt -ge $MaxAttempts) {
            throw "Failed after $MaxAttempts attempts (last exit code: $exit)"
        }
        Warn "Command failed (attempt $attempt/$MaxAttempts, exit $exit). Retrying in ${delay}s..."
        Start-Sleep -Seconds $delay
        $delay = $delay * 2
    }
}

function Show-Summary {
    Write-Host ""
    Write-Host "Luminus-Proxy" -ForegroundColor Cyan -NoNewline
    Write-Host " - Unified AI provider proxy"
    Write-Host ""

    $needsGit = -not (Have git)
    $needsBun = -not (Get-BunCommand)

    $hasRealPython = $false
    foreach ($cand in @("python3.12","python3.11","python","python3")) {
        if (Have $cand) {
            if (Test-RealPython $cand) { $hasRealPython = $true; break }
        }
    }
    $needsPython = -not $hasRealPython

    $totalSize = 0
    $items = @()

    if ($needsGit)    { $items += "  * Git                          ~50 MB";  $totalSize += 50  }
    if ($needsBun)    { $items += "  * Bun runtime                  ~50 MB";  $totalSize += 50  }
    if ($needsPython) { $items += "  * Python 3.10+                 ~100 MB"; $totalSize += 100 }

    $items += "  * Node.js dependencies         ~200 MB"; $totalSize += 200
    $items += "  * Python packages (venv)       ~150 MB"; $totalSize += 150
    if ($env:LUMINUS_SKIP_BROWSERS -ne "1") {
        $items += "  * Playwright Chromium          ~175 MB"; $totalSize += 175
        $items += "  * Camoufox browser             ~150 MB"; $totalSize += 150
    }
    $items += "  * Dashboard build              ~50 MB";  $totalSize += 50

    Write-Host "This will install:" -ForegroundColor White
    foreach ($item in $items) { Write-Host $item }
    Write-Host ""
    Write-Host "Estimated total size: " -NoNewline; Write-Host "~$totalSize MB" -ForegroundColor Yellow
    Write-Host "Install location:     $DefaultDir"
    Write-Host "PowerShell version:   $($PSVersionTable.PSVersion)"
    Write-Host ""

    if ($needsGit -or $needsBun -or $needsPython) {
        Write-Host "Note: " -ForegroundColor Yellow -NoNewline
        Write-Host "System dependencies will be installed via package manager (winget/scoop/choco)."
        Write-Host "      This may require " -NoNewline; Write-Host "administrator privileges" -ForegroundColor Yellow -NoNewline; Write-Host "."
        Write-Host ""
    }

    if ($AssumeYes) {
        Write-Host "LUMINUS_YES=1 set - skipping confirmation." -ForegroundColor DarkGray
        Write-Host ""
        return
    }

    if (-not [Environment]::UserInteractive) {
        Write-Host "Non-interactive shell - proceeding automatically." -ForegroundColor DarkGray
        Write-Host ""
        return
    }

    $response = Read-Host "Do you want to continue? [Y/n]"
    if ($response -match '^[nN]') {
        Write-Host "Installation cancelled." -ForegroundColor Yellow
        exit 0
    }
    Write-Host ""
}

function Ensure-PackageManager {
    # Need at least one of: winget, scoop, choco
    if ((Have winget) -or (Have scoop) -or (Have choco)) { return }

    Step "Installing Scoop (no winget/choco found)"
    try {
        Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser -Force
        Invoke-RestMethod get.scoop.sh | Invoke-Expression
        Add-PathOnce (Join-Path $HOME "scoop\shims")
        Refresh-Path
        if (-not (Have scoop)) {
            Fail "Scoop install completed but 'scoop' is not on PATH. Open a new PowerShell and re-run."
        }
        Ok "Scoop installed"
    } catch {
        Fail @"
No package manager (winget / scoop / choco) was found and Scoop install failed.
Install one of these manually, then re-run:
  * winget  - built into Windows 10/11; update from Microsoft Store
  * scoop   - https://scoop.sh
  * choco   - https://chocolatey.org/install
"@
    }
}

function Ensure-Git {
    if (Have git) { Ok "Git $(git --version | ForEach-Object { ($_ -split ' ')[2] }) already installed"; return }
    Step "Installing Git"
    if (Have winget) {
        Invoke-Native -Quiet { winget install --id Git.Git --silent --accept-package-agreements --accept-source-agreements } | Out-Null
    } elseif (Have scoop) {
        Invoke-Native -Quiet { scoop install git } | Out-Null
    } elseif (Have choco) {
        Invoke-Native -Quiet { choco install -y git } | Out-Null
    } else {
        Fail "Install Git manually from https://git-scm.com/download/win and re-run this script"
    }
    Refresh-Path
    Add-PathOnce "$env:ProgramFiles\Git\cmd"
    Add-PathOnce "${env:ProgramFiles(x86)}\Git\cmd"
    Add-PathOnce "$env:LOCALAPPDATA\Programs\Git\cmd"
    if (-not (Have git)) { Fail "git is still not on PATH. Open a new PowerShell window and re-run." }
    Ok "Git installed"
}

function Ensure-Bun {
    $existing = Get-BunCommand
    if ($existing) {
        $prev = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $ver = & $existing --version 2>&1 | Out-String
        $ErrorActionPreference = $prev
        Ok "Bun $($ver.Trim()) already installed ($existing)"
        $script:BunCmd = $existing
        return
    }
    Step "Installing Bun (official bun.exe)"
    try {
        Invoke-Native -Quiet { powershell -NoProfile -Command "irm https://bun.sh/install.ps1 | iex" } | Out-Null
    } catch {
        Fail "Bun install failed: $_`nIf you're behind a corporate proxy, set HTTPS_PROXY first."
    }
    Add-PathOnce (Join-Path $HOME ".bun\bin")
    Add-PathOnce (Join-Path $env:USERPROFILE ".bun\bin")
    Refresh-Path
    # Official installer puts bun.exe here - ensure it wins over any extensionless shim
    $official = Join-Path (Join-Path $env:USERPROFILE ".bun") "bin"
    Add-PathOnce $official
    $found = Get-BunCommand
    if (-not $found) {
        # Last resort: call bun.exe by absolute path if the installer dropped it
        $abs = Join-Path $official "bun.exe"
        if (Test-Path -LiteralPath $abs) {
            $script:BunCmd = $abs
            Ok "Bun $(& $abs --version) installed at $abs"
            return
        }
        Warn "Bun installed but bun.exe is not on PATH yet. Open a new PowerShell and re-run this installer."
        Info "  Expected location: $abs"
        exit 1
    }
    $script:BunCmd = $found
    Ok "Bun $(& $found --version) installed"
}

function Ensure-Python {
    $script:PythonBin = $null
    foreach ($cand in @("python3.13","python3.12","python3.11","python3.10","python","python3")) {
        if (Have $cand) {
            if (-not (Test-RealPython $cand)) {
                Warn "$cand looks like the Microsoft Store stub - skipping"
                continue
            }
            try {
                $prev = $ErrorActionPreference
                $ErrorActionPreference = 'Continue'
                $ver = & $cand -c "import sys;print('{0}.{1}'.format(*sys.version_info[:2]))" 2>$null
                $ErrorActionPreference = $prev
                if ($ver) {
                    $parts = $ver.ToString().Trim().Split('.')
                    if ([int]$parts[0] -ge 3 -and [int]$parts[1] -ge 10) {
                        $script:PythonBin = $cand
                        Ok "Python $ver found ($cand)"
                        return
                    }
                }
            } catch {
                if ($null -ne $prev) { $ErrorActionPreference = $prev }
            }
        }
    }
    Step "Installing Python 3.11"
    if (Have winget) {
        Invoke-Native -Quiet { winget install --id Python.Python.3.11 --silent --accept-package-agreements --accept-source-agreements } | Out-Null
    } elseif (Have scoop) {
        Invoke-Native -Quiet { scoop install python } | Out-Null
    } elseif (Have choco) {
        Invoke-Native -Quiet { choco install -y python --version=3.11 } | Out-Null
    } else {
        Fail "Install Python 3.10+ manually from https://python.org and re-run"
    }
    Refresh-Path
    foreach ($cand in @("python3.11","python","python3")) {
        if ((Have $cand) -and (Test-RealPython $cand)) { $script:PythonBin = $cand; break }
    }
    if (-not $script:PythonBin) {
        Warn "Python installed but not on PATH yet. Open a new PowerShell and re-run."
        exit 1
    }
    Ok "Python $(& $script:PythonBin --version) installed"
}

function Clone-Or-Update-Repo {
    $script:ProjectDir = $null
    if (Test-Path "package.json") {
        $pkg = Get-Content "package.json" -Raw
        if ($pkg -match '"name"\s*:\s*"luminus-pool"') {
            $script:ProjectDir = (Get-Location).Path
            Step "Using existing checkout: $($script:ProjectDir)"
            if (Test-Path ".git") {
                try { git pull --ff-only | Out-Null } catch { Warn "git pull failed (continuing)" }
            }
            return
        }
    }

    if (Test-Path (Join-Path $DefaultDir ".git")) {
        $script:ProjectDir = $DefaultDir
        Step "Updating existing checkout at $($script:ProjectDir)"
        Push-Location $script:ProjectDir
        try { git pull --ff-only | Out-Null } catch { Warn "git pull failed" }
        finally { Pop-Location }
    } else {
        $script:ProjectDir = $DefaultDir
        Step "Cloning $RepoUrl -> $($script:ProjectDir) (branch: $Branch)"
        git clone --depth=1 --branch $Branch $RepoUrl $script:ProjectDir
        if ($LASTEXITCODE -ne 0) {
            Fail "git clone failed. Check connectivity and repo URL: $RepoUrl"
        }
    }
    Set-Location $script:ProjectDir
}

function Write-EnvIfMissing {
    Step "Configuring .env"
    if (Test-Path ".env") {
        Info ".env already exists, checking for missing keys..."
    } else {
        Copy-Item ".env.example" ".env"
        Info "Created .env from .env.example"
    }

    $envContent = Get-Content ".env" -Raw

    # Generate ENCRYPTION_KEY if it's still the default placeholder
    if ($envContent -match 'ENCRYPTION_KEY=a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6' -or $envContent -match 'ENCRYPTION_KEY=\s*$' -or $envContent -notmatch 'ENCRYPTION_KEY=') {
        $bytes = New-Object byte[] 16
        [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
        $key = ($bytes | ForEach-Object { $_.ToString("x2") }) -join ""

        if ($envContent -match 'ENCRYPTION_KEY=') {
            (Get-Content ".env") -replace '^ENCRYPTION_KEY=.*', "ENCRYPTION_KEY=$key" | Set-Content ".env"
        } else {
            Add-Content ".env" "ENCRYPTION_KEY=$key"
        }
        Ok "Generated random ENCRYPTION_KEY"
    }

    # Auto-rotate API_KEY off the default
    $envContent = Get-Content ".env" -Raw
    if ($envContent -match 'API_KEY=pool-proxy-secret-key') {
        $bytes = New-Object byte[] 24
        [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
        $newApi = ($bytes | ForEach-Object { $_.ToString("x2") }) -join ""
        (Get-Content ".env") -replace '^API_KEY=.*', "API_KEY=$newApi" | Set-Content ".env"
        Ok "Generated random API_KEY"
        Info "  Your API key: $newApi"
        Info "  Clients send this as: Authorization: Bearer <api_key>"
    }

    # PYTHON_PATH should be empty (auto-detect) - server picks the venv path per-OS at runtime
    $envContent = Get-Content ".env" -Raw
    if ($envContent -notmatch 'PYTHON_PATH=') {
        Add-Content ".env" "PYTHON_PATH="
        Info "Added PYTHON_PATH= (auto-detect)"
    } else {
        $pyPath = ((Get-Content ".env") | Where-Object { $_ -match '^PYTHON_PATH=' }) -replace '^PYTHON_PATH=', ''
        if ($pyPath -and -not (Test-Path $pyPath)) {
            Warn "PYTHON_PATH=$pyPath does not exist - clearing for auto-detect"
            (Get-Content ".env") -replace '^PYTHON_PATH=.*', 'PYTHON_PATH=' | Set-Content ".env"
        }
    }

    # Ensure other required keys exist
    $envContent = Get-Content ".env" -Raw
    $requiredKeys = @("PORT", "DASHBOARD_PORT", "API_KEY", "DATABASE_PATH", "AUTH_SCRIPT_PATH", "AUTH_SCRIPT_CWD")
    foreach ($keyName in $requiredKeys) {
        if ($envContent -notmatch "(?m)^${keyName}=") {
            $defaultVal = ""
            if (Test-Path ".env.example") {
                $exLine = (Get-Content ".env.example") | Where-Object { $_ -match "^${keyName}=" }
                if ($exLine) { $defaultVal = $exLine -replace "^${keyName}=", "" }
            }
            Add-Content ".env" "${keyName}=${defaultVal}"
            Info "Added missing ${keyName}"
        }
    }
}

function Install-NodeDeps {
    Step "Installing JS dependencies"
    if (-not $script:BunCmd) {
        $script:BunCmd = Get-BunCommand
    }
    if (-not $script:BunCmd) {
        Add-PathOnce (Join-Path $HOME ".bun\bin")
        Add-PathOnce (Join-Path $env:USERPROFILE ".bun\bin")
        $script:BunCmd = Get-BunCommand
    }
    if (-not $script:BunCmd) {
        Fail "bun.exe is not on PATH. Open a new PowerShell and re-run the installer."
    }
    $bun = $script:BunCmd

    Info "Installing root dependencies..."
    try {
        Retry-Action -Action { & $bun install }
    } catch {
        Fail "bun install failed in project root. Try manually: $bun install"
    }

    Info "Installing dashboard dependencies..."
    Push-Location "dashboard"
    try {
        try {
            Retry-Action -Action { & $bun install }
        } catch {
            Fail "bun install failed in dashboard/. Try manually: cd dashboard; $bun install"
        }
    } finally {
        Pop-Location
    }

    Ok "JS dependencies installed"
}

function Setup-PythonVenv {
    # PS 5.1 Join-Path only accepts 2 args - nest for multi-segment paths
    $venv = Join-Path (Join-Path "scripts" "auth") ".venv"
    $venvPip = Join-Path (Join-Path $venv "Scripts") "pip.exe"
    $venvPy  = Join-Path (Join-Path $venv "Scripts") "python.exe"

    Step "Setting up Python venv at $venv"

    if (-not (Test-Path $venv)) {
        Info "Creating virtual environment..."
        & $script:PythonBin -m venv $venv
        if ($LASTEXITCODE -ne 0) {
            Fail "Failed to create Python venv at $venv. Try manually: $($script:PythonBin) -m venv $venv"
        }
    }

    if (-not (Test-Path $venvPy)) {
        Fail "Python venv created but $venvPy not found! Try deleting $venv and re-running the installer."
    }
    if (-not (Test-Path $venvPip)) {
        Fail "Python venv created but pip not found at $venvPip. Try deleting $venv and re-running."
    }

    Info "Upgrading pip..."
    $pipUp = Invoke-Native -Quiet { & $venvPip install --upgrade pip wheel }
    if ($pipUp -ne 0) {
        Warn "pip upgrade returned exit $pipUp - continuing"
    }

    Info "Installing Python packages (this may take a minute)..."
    $reqFile = Join-Path (Join-Path "scripts" "auth") "requirements.txt"
    try {
        Retry-Action -Action { & $venvPip install -r $reqFile }
    } catch {
        Fail "pip install failed. Try manually: $venvPip install -r scripts\auth\requirements.txt"
    }
    Ok "Python deps installed"

    if ($env:LUMINUS_SKIP_BROWSERS -eq "1") {
        Warn "LUMINUS_SKIP_BROWSERS=1 - skipping Playwright/Camoufox download."
        Warn "  Auth bot will fail until you run: $venvPy -m playwright install chromium && $venvPy -m camoufox fetch"
        return
    }

    Step "Installing browsers (Playwright + Camoufox - this can take a few minutes)"
    Info "Installing Playwright Chromium..."
    try {
        Retry-Action -Action { & $venvPy -m playwright install chromium }
        Ok "Playwright Chromium installed"
    } catch {
        Warn "Playwright Chromium install failed (re-run later)"
        Info "  Manual: $venvPy -m playwright install chromium"
    }

    Info "Fetching Camoufox browser..."
    try {
        Retry-Action -Action { & $venvPy -m camoufox fetch }
        Ok "Camoufox browser installed"
    } catch {
        Warn "Camoufox fetch failed (re-run later)"
        Info "  Manual: $venvPy -m camoufox fetch"
    }
}

function Build-Dashboard {
    Step "Building dashboard (production)"
    $bun = $script:BunCmd
    if (-not $bun) { $bun = Get-BunCommand }
    if (-not $bun) { Fail "bun.exe not found - cannot build dashboard" }
    Push-Location "dashboard"
    try {
        try {
            Retry-Action -Action { & $bun run build }
        } catch {
            Fail "Dashboard build failed. Try manually: cd dashboard; $bun run build"
        }
    } finally {
        Pop-Location
    }
    Ok "Dashboard built"
}

function Run-Migrations {
    Step "Running database migrations"
    $bun = $script:BunCmd
    if (-not $bun) { $bun = Get-BunCommand }
    if (-not (Test-Path "data")) { New-Item -ItemType Directory -Path "data" -Force | Out-Null }
    try {
        if ($bun) {
            Invoke-Native { & $bun src/db/migrate.ts } | Out-Null
        } else {
            throw "bun not found"
        }
        if ($LASTEXITCODE -eq 0) {
            Ok "Migrations applied"
        } else {
            Warn "Migrations failed. Database will be created on first run."
            Info "After first run, you can re-run: bun src/db/migrate.ts"
        }
    } catch {
        Warn "Migrations failed. Database will be created on first run."
        Info "After first run, you can re-run: bun src/db/migrate.ts"
    }
}

function Install-CliShims {
    if ($env:LUMINUS_NO_CLI -eq "1") {
        Warn "LUMINUS_NO_CLI=1 - skipping CLI install"
        return
    }
    Step "Installing CLI commands"
    $target = Join-Path $HOME ".local\bin"
    if (-not (Test-Path $target)) {
        New-Item -ItemType Directory -Path $target -Force | Out-Null
    }

    $srcPs1 = Join-Path $script:ProjectDir "luminus.ps1"
    $srcCmd = Join-Path $script:ProjectDir "luminus.cmd"

    if (Test-Path $srcPs1) {
        Copy-Item $srcPs1 (Join-Path $target "luminus.ps1") -Force
    } else {
        Warn "luminus.ps1 not found at $srcPs1"
    }
    if (Test-Path $srcCmd) {
        Copy-Item $srcCmd (Join-Path $target "luminus.cmd") -Force
    } else {
        Warn "luminus.cmd not found at $srcCmd"
    }

    Ok "Installed luminus command to $target"

    if (($env:Path -split ';') -notcontains $target) {
        Warn "$target is not on your PATH."
        Info "Add it for this session:"
        Info "  `$env:Path = `"$target;`$env:Path`""
        Info "Or permanently:"
        Info "  setx Path `"$target;%Path%`""
    }
}

function Run-Preflight {
    Step "Running preflight check"
    $bun = $script:BunCmd
    if (-not $bun) { $bun = Get-BunCommand }
    try {
        if ($bun) {
            $code = Invoke-Native { & $bun scripts/preflight.ts }
            if ($code -eq 0) { return }
        }
    } catch {}
    Warn "Preflight reported issues - see above. The server may still start."
    Info "Run 'luminus doctor' for a detailed report."
}

function Main {
    Write-Host ""
    Write-Host "Luminus Pool Installer (Windows)" -ForegroundColor Blue
    Write-Host ""

    Show-Summary

    Ensure-PackageManager
    Ensure-Git
    Ensure-Bun
    Ensure-Python
    Clone-Or-Update-Repo

    Set-Location $script:ProjectDir
    Write-EnvIfMissing
    Install-NodeDeps
    Setup-PythonVenv
    Build-Dashboard
    Run-Migrations
    Install-CliShims
    Run-Preflight

    Write-Host ""
    Write-Host "Installation complete!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Luminus Pool is installed at: $($script:ProjectDir)"
    Write-Host ""

    Write-Host "Quick Start:" -ForegroundColor White -BackgroundColor DarkBlue
    Write-Host ""
    Write-Host "  1. Start the server:" -ForegroundColor Cyan
    Write-Host "     luminus start"
    Write-Host "     (or: cd $($script:ProjectDir); .\luminus.ps1 start)"
    Write-Host ""
    Write-Host "  2. Open the dashboard:" -ForegroundColor Cyan
    Write-Host "     http://localhost:1931"
    Write-Host ""
    Write-Host "  3. Add accounts via the dashboard UI"
    Write-Host ""

    Write-Host "Useful Commands:" -ForegroundColor White -BackgroundColor DarkBlue
    Write-Host ""
    Write-Host "  luminus status     Check server status"
    Write-Host "  luminus logs       View server logs"
    Write-Host "  luminus stop       Stop the server"
    Write-Host "  luminus restart    Restart the server"
    Write-Host "  luminus doctor     Diagnose installation health"
    Write-Host "  luminus update     Pull latest, rebuild, restart"
    Write-Host "  luminus help       Full command reference"
    Write-Host ""

    Write-Host "Tip: re-run this installer any time to pull updates and rebuild." -ForegroundColor Gray
    Write-Host "Tip: trouble? run `luminus doctor` to get a checklist of fixes." -ForegroundColor Gray
}

Main
