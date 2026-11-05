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

function Get-UserPath { [Environment]::GetEnvironmentVariable('Path', 'User') }

function Remove-FromUserPath {
    $parts = @((Get-UserPath) -split ';' | Where-Object { $_ -and $_.TrimEnd('\') -ne $dir.TrimEnd('\') })
    [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User')
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
$onPath = @((Get-UserPath) -split ';' | ForEach-Object { $_.TrimEnd('\') }) -contains $dir.TrimEnd('\')
if ($onPath) {
    # already there
}
elseif ($env:EV_NO_MODIFY_PATH -eq '1') {
    Write-Host "$dir is not on your PATH. Add it yourself to your user PATH."
}
else {
    $current = Get-UserPath
    if ($current) { $new = "$current;$dir" } else { $new = $dir }
    [Environment]::SetEnvironmentVariable('Path', $new, 'User')
    Write-Host "Added $dir to your user PATH."
}

Write-Host ""
Write-Host "Open a new terminal window, then run:   ev --help"
Write-Host "Variables you set with ev (ev set NAME value) are stored in your user environment and apply to new terminals."
