# Builds the Android remote into a signed APK, with the Android SDK's own tools (no Gradle):
#   1. the Rust library for each kind of phone (arm64 and the older 32-bit arm), linked with the NDK's clang;
#   2. aapt2 makes the APK from AndroidManifest.xml and res/; the libraries go in lib/<kind>/;
#   3. zipalign, then apksigner signs it.
#
#   build-apk.ps1 [-Out <file.apk>] [-Abis arm64-v8a,armeabi-v7a] [-Debug]
#
# Finds the Android SDK in ANDROID_HOME / ANDROID_SDK_ROOT, else .tools\android\sdk in the repo (or in the main
# checkout of a git worktree); the NDK in ANDROID_NDK_HOME, else the newest under <sdk>\ndk. Signs with the key in
# ANDROID_KEYSTORE (+ ANDROID_KEYSTORE_PASSWORD, ANDROID_KEY_ALIAS) when set, else with a debug key made once in
# %LOCALAPPDATA%\RustyPresenter (or ~/.android). Works in Windows PowerShell and pwsh (Linux, for CI).
param(
    [string]$Out = "",
    [string[]]$Abis = @("arm64-v8a", "armeabi-v7a"),
    [switch]$Debug
)
# Native tools write their progress to stderr: Windows PowerShell would stop on it, so each exit code is checked instead.
$ErrorActionPreference = "Continue"
$App = $PSScriptRoot
$Repo = (Resolve-Path "$App\..\..").Path
$OnWindows = ($env:OS -eq "Windows_NT")
function Say($m) { Write-Host "build-apk: $m" }
function Die($m) { Write-Host "build-apk: $m" -ForegroundColor Red; exit 1 }

# ---- where the tools are ----
$Tools = $null
foreach ($root in @($Repo, (git -C $Repo rev-parse --path-format=absolute --git-common-dir 2>$null | Split-Path -Parent))) {
    if ($root -and (Test-Path (Join-Path $root ".tools"))) { $Tools = Join-Path $root ".tools"; break }
}
$Sdk = $env:ANDROID_HOME
if (-not $Sdk) { $Sdk = $env:ANDROID_SDK_ROOT }
if (-not $Sdk -and $Tools) { $Sdk = Join-Path $Tools "android/sdk" }
if (-not $Sdk -or -not (Test-Path $Sdk)) { Die "no Android SDK: set ANDROID_HOME" }
$env:ANDROID_HOME = $Sdk
$Ndk = $env:ANDROID_NDK_HOME
if (-not $Ndk) { $Ndk = (Get-ChildItem (Join-Path $Sdk "ndk") -Directory | Sort-Object Name | Select-Object -Last 1).FullName }
if (-not $Ndk -or -not (Test-Path $Ndk)) { Die "no NDK: set ANDROID_NDK_HOME" }
$env:ANDROID_NDK_HOME = $Ndk
$Bt = (Get-ChildItem (Join-Path $Sdk "build-tools") -Directory | Sort-Object { [version]($_.Name -replace '[^0-9.]', '') } | Select-Object -Last 1).FullName
$Platform = (Get-ChildItem (Join-Path $Sdk "platforms") -Directory | Sort-Object { [int]($_.Name -replace '\D', '') } | Select-Object -Last 1).FullName
$AndroidJar = Join-Path $Platform "android.jar"
if (-not (Test-Path $AndroidJar)) { Die "no android.jar in $Platform" }
$Exe = ""; $Cmd = ""; $Bat = ""
if ($OnWindows) { $Exe = ".exe"; $Cmd = ".cmd"; $Bat = ".bat" }
$Host_ = "linux-x86_64"
if ($OnWindows) { $Host_ = "windows-x86_64" } elseif ($IsMacOS) { $Host_ = "darwin-x86_64" }
$Llvm = Join-Path $Ndk "toolchains/llvm/prebuilt/$Host_/bin"

# The repo's own Rust (the PC set-up) when there is one and none is set.
if ($Tools -and (Test-Path (Join-Path $Tools "cargo/bin")) -and -not $env:RUSTUP_HOME) {
    $env:CARGO_HOME = Join-Path $Tools "cargo"; $env:RUSTUP_HOME = Join-Path $Tools "rustup"
    $env:PATH = (Join-Path $Tools "cargo/bin") + [IO.Path]::PathSeparator + $env:PATH
    # The PC's Rust is the MinGW one: its build scripts need MinGW's dlltool.
    if (Test-Path (Join-Path $Tools "mingw64/bin")) { $env:PATH = (Join-Path $Tools "mingw64/bin") + [IO.Path]::PathSeparator + $env:PATH }
}
if (-not $env:CARGO_TARGET_DIR) {
    if ($env:LOCALAPPDATA) { $env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "RustyPresenter/build-android" }
    else { $env:CARGO_TARGET_DIR = Join-Path $App "target" }
}
# The Java the Slint library needs to build its small helper.
if (-not $env:JAVA_HOME) {
    $javac = (Get-Command javac -ErrorAction SilentlyContinue).Source
    if ($javac) {
        $real = (Get-Item $javac).Target
        if (-not $real) { $real = $javac }
        $env:JAVA_HOME = Split-Path (Split-Path $real -Parent) -Parent
        if ($OnWindows -and -not (Test-Path (Join-Path $env:JAVA_HOME "bin/keytool.exe"))) {
            $jdk = Get-ChildItem "C:\Program Files\Java" -Directory -Filter "jdk*" -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -Last 1
            if ($jdk) { $env:JAVA_HOME = $jdk.FullName }
        }
    }
}
$Keytool = "keytool"
if ($env:JAVA_HOME) { $Keytool = Join-Path $env:JAVA_HOME "bin/keytool$Exe" }

$Version = (Select-String -Path (Join-Path $App "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$parts = $Version.Split('.') | ForEach-Object { [int]($_ -replace '\D', '') }
$VersionCode = $parts[0] * 10000 + $parts[1] * 100 + $parts[2]
if (-not $Out) { $Out = Join-Path $env:CARGO_TARGET_DIR "RustyPresenter-Remote-$Version.apk" }
Say "version $Version ($VersionCode), SDK $Sdk, NDK $(Split-Path $Ndk -Leaf), build-tools $(Split-Path $Bt -Leaf)"

# ---- 1. the Rust library for each kind of phone ----
$Profile = "release"
if ($Debug) { $Profile = "debug" }
$Targets = @{ "arm64-v8a" = "aarch64-linux-android"; "armeabi-v7a" = "armv7-linux-androideabi"; "x86_64" = "x86_64-linux-android" }
$Clang = @{ "arm64-v8a" = "aarch64-linux-android26-clang"; "armeabi-v7a" = "armv7a-linux-androideabi26-clang"; "x86_64" = "x86_64-linux-android26-clang" }
$Libs = @{}
Push-Location $App
try {
    foreach ($abi in $Abis) {
        $t = $Targets[$abi]
        if (-not $t) { Die "unknown kind of phone: $abi" }
        $cc = Join-Path $Llvm "$($Clang[$abi])$Cmd"
        $up = $t.ToUpper().Replace('-', '_')
        Set-Item "env:CARGO_TARGET_${up}_LINKER" $cc
        Set-Item "env:CC_$($t.Replace('-', '_'))" $cc
        Set-Item "env:AR_$($t.Replace('-', '_'))" (Join-Path $Llvm "llvm-ar$Exe")
        Say "building for $abi ($t)"
        $cargoArgs = @("build", "--lib", "--features", "phone", "--target", $t)
        if (-not $Debug) { $cargoArgs += "--release" }
        if ($env:RP_OFFLINE) { $cargoArgs += "--offline" }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { Die "cargo build failed for $abi" }
        $Libs[$abi] = Join-Path $env:CARGO_TARGET_DIR "$t/$Profile/libremote.so"
    }
} finally { Pop-Location }

# ---- 2. the APK ----
$Work = Join-Path $env:CARGO_TARGET_DIR "apk"
if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
New-Item -ItemType Directory -Force (Join-Path $Work "res") | Out-Null
& (Join-Path $Bt "aapt2$Exe") compile --dir (Join-Path $App "android/res") -o (Join-Path $Work "res.zip")
if ($LASTEXITCODE -ne 0) { Die "aapt2 compile failed" }
$Manifest = Join-Path $Work "AndroidManifest.xml"
Copy-Item (Join-Path $App "android/AndroidManifest.xml") $Manifest
$Unsigned = Join-Path $Work "unsigned.apk"
& (Join-Path $Bt "aapt2$Exe") link -o $Unsigned -I $AndroidJar --manifest $Manifest (Join-Path $Work "res.zip") `
    --min-sdk-version 26 --target-sdk-version 34 --version-code $VersionCode --version-name $Version
if ($LASTEXITCODE -ne 0) { Die "aapt2 link failed" }
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::Open($Unsigned, [IO.Compression.ZipArchiveMode]::Update)
try {
    foreach ($abi in $Abis) {
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $Libs[$abi], "lib/$abi/libremote.so", [IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
} finally { $zip.Dispose() }
$Aligned = Join-Path $Work "aligned.apk"
& (Join-Path $Bt "zipalign$Exe") -f -p 4 $Unsigned $Aligned
if ($LASTEXITCODE -ne 0) { Die "zipalign failed" }

# ---- 3. signing ----
$Ks = $env:ANDROID_KEYSTORE; $KsPass = $env:ANDROID_KEYSTORE_PASSWORD; $Alias = $env:ANDROID_KEY_ALIAS
if (-not $Ks) {
    $home_ = $env:LOCALAPPDATA
    if ($home_) { $home_ = Join-Path $home_ "RustyPresenter" } else { $home_ = Join-Path $HOME ".android" }
    New-Item -ItemType Directory -Force $home_ | Out-Null
    $Ks = Join-Path $home_ "remote-debug.keystore"; $KsPass = "android"; $Alias = "androiddebugkey"
    if (-not (Test-Path $Ks)) {
        Say "making a debug signing key in $Ks"
        & $Keytool -genkeypair -keystore $Ks -storepass $KsPass -keypass $KsPass -alias $Alias -keyalg RSA -keysize 2048 `
            -validity 10000 -dname "CN=RustyPresenter debug, O=RustyPresenter, C=US" | Out-Null
        if ($LASTEXITCODE -ne 0) { Die "keytool failed" }
    }
}
if (-not $Alias) { $Alias = "remote" }
$OutDir = Split-Path $Out -Parent
if ($OutDir) { New-Item -ItemType Directory -Force $OutDir | Out-Null }
& (Join-Path $Bt "apksigner$Bat") sign --ks $Ks --ks-pass "pass:$KsPass" --key-pass "pass:$KsPass" --ks-key-alias $Alias --out $Out $Aligned
if ($LASTEXITCODE -ne 0) { Die "apksigner failed" }
& (Join-Path $Bt "apksigner$Bat") verify $Out
if ($LASTEXITCODE -ne 0) { Die "the APK's signature does not check" }
Say "done: $Out"
