# ev installer for Windows (PowerShell 5.1+).
#
#   irm https://github.com/FahimSifnatul/ev/releases/latest/download/install.ps1 | iex
#
# Uninstall (iex cannot take arguments, so use an environment variable):
#   $env:EV_UNINSTALL = '1'; irm https://github.com/FahimSifnatul/ev/releases/latest/download/install.ps1 | iex
#
# Optional environment variables:
#   EV_INSTALL_DIR      where to put ev.exe                  (default: %LOCALAPPDATA%\Programs\ev)
#   EV_VERSION          release tag such as v0.1.0           (default: the latest release)
#   EV_NO_MODIFY_PATH=1 do not add the folder to your user PATH
#   EV_UNINSTALL=1      remove ev again
#   EV_REPO, EV_BASE_URL  for forks and mirrors

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

function Env-Or($name, $default) {
    $value = [Environment]::GetEnvironmentVariable($name)
    if ($value) { $value } else { $default }
}

$repo    = Env-Or 'EV_REPO' 'FahimSifnatul/ev'
$base    = Env-Or 'EV_BASE_URL' 'https://github.com'
$version = Env-Or 'EV_VERSION' 'latest'
$dir     = Env-Or 'EV_INSTALL_DIR' (Join-Path $env:LOCALAPPDATA 'Programs\ev')
$exe     = Join-Path $dir 'ev.exe'

# Read the raw user PATH (keeps %VARS% unexpanded) and write it back as REG_EXPAND_SZ,
# so existing entries such as %USERPROFILE%\bin keep working.
function Get-UserPath {
    (Get-Item 'HKCU:\Environment').GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
}

# The non-empty PATH entries, exactly as stored (never trimmed: `C:\` must stay `C:\`). Callers wrap
# the call in @() so that zero or one entry still behaves as an array.
function Get-UserPathParts {
    (Get-UserPath) -split ';' | Where-Object { $_ }
}

# Compare two folders, ignoring case (PowerShell's default) and trailing backslashes. Comparison only.
function Test-SameDir($a, $b) { $a.TrimEnd('\') -eq $b.TrimEnd('\') }

function Set-UserPath($value) {
    Set-ItemProperty -Path 'HKCU:\Environment' -Name 'Path' -Value $value -Type ExpandString
    # setting and clearing a user variable makes Windows broadcast the environment change
    [Environment]::SetEnvironmentVariable('EV_REFRESH', '1', 'User')
    [Environment]::SetEnvironmentVariable('EV_REFRESH', $null, 'User')
}

function Remove-FromUserPath {
    $parts = @(Get-UserPathParts)
    $kept  = @($parts | Where-Object { -not (Test-SameDir $_ $dir) })
    if ($kept.Count -ne $parts.Count) { Set-UserPath ($kept -join ';') }
}

# ---------------------------------------------------------------- uninstall
if ($env:EV_UNINSTALL -eq '1') {
    if (-not (Test-Path $exe)) { throw "ev is not installed (looked for $exe)." }
    # only delete a file that really is ev, never some other program with the same name
    $out = & $exe --version 2>$null
    if (-not ("$out" -like 'ev *')) { throw "$exe does not look like ev, leaving it alone." }
    Remove-Item $exe -Force
    Remove-FromUserPath
    Write-Host "Removed $exe and its PATH entry."
    Write-Host "(Your variables are left in place; they live in HKCU\Environment.)"
    return
}

# ---------------------------------------------------------------- download
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
    throw "Only 64-bit Intel/AMD Windows (x86_64) has a ready-made ev. Build from source with cargo instead."
}

$asset = 'ev-windows-x86_64.exe'
if ($version -eq 'latest') { $rel = "$base/$repo/releases/latest/download" }
else { $rel = "$base/$repo/releases/download/$version" }

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("ev-install-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $file = Join-Path $tmp $asset
    Write-Host "Downloading ev ($asset)..."
    Invoke-WebRequest -UseBasicParsing -Uri "$rel/$asset" -OutFile $file
    Invoke-WebRequest -UseBasicParsing -Uri "$rel/$asset.sha256" -OutFile "$file.sha256"

    $want = ((Get-Content "$file.sha256" -Raw).Trim() -split '\s+')[0]
    $got  = (Get-FileHash -Algorithm SHA256 $file).Hash
    if ($got -ne $want) { throw "the download is corrupted (checksum mismatch). Nothing was installed." }

    $out = & $file --version 2>$null
    if (-not ("$out" -like 'ev *')) { throw "the downloaded file does not run, nothing was installed." }

    # ------------------------------------------------------------ install
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Copy-Item $file $exe -Force
    Write-Host "Installed $(& $exe --version) at $exe"
}
finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------- put it on the PATH
$parts = @(Get-UserPathParts)
if (@($parts | Where-Object { Test-SameDir $_ $dir }).Count -gt 0) {
    # already there
}
elseif ($env:EV_NO_MODIFY_PATH -eq '1') {
    Write-Host "$dir is not on your PATH. Add it yourself to your user PATH."
}
else {
    Set-UserPath (($parts + $dir) -join ';')
    Write-Host "Added $dir to your user PATH."
}

Write-Host ""
Write-Host "Open a new terminal window, then run:   ev --help"
Write-Host "Variables you set with ev (ev set NAME value) are stored in your user environment and apply to new terminals."
