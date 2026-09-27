# Il pacchetto da distribuire di NicoReader (versione Rust), per Windows.
#
# Uso (nella cartella rust):
#   powershell -ExecutionPolicy Bypass -File pacchetto.ps1 [-SenzaProve] [-Arch x64|arm64]
#
# Senza -Arch, per l'architettura del PC su cui gira. -Arch arm64 fa la
# versione per Windows su ARM (Snapdragon X, Surface Pro e simili): da un PC
# x64 si compila lo stesso, ma servono gli strumenti ARM64 di Visual Studio
# ("MSVC ... ARM64 build tools") e `rustup target add aarch64-pc-windows-msvc`,
# e le prove girano solo su un PC ARM. Per arm64 nasm non serve.
#
# Fa, in ordine:
#   1. pdfium per ARM64 in vendor\pdfium\win-arm64, se manca (dal pacchetto
#      di pdfium-binaries della stessa versione di quella x64, controllato
#      con SHA-256); quella x64 sta gia' nel repository
#   2. le prove (cargo test --workspace), se non si dice -SenzaProve
#   3. l'eseguibile (cargo build --release)
#   4. dist\NicoReader\: NicoReader.exe, pdfium.dll (per i PDF) e la cartella
#      licenze\ con le licenze di tutto cio' che sta dentro
#   5. dist\NicoReader-<versione>-windows-<arch>.zip, da mettere nelle Release
#   6. dist\NicoReader-<versione>-windows-<arch>-setup.exe, l'installer
#      (installer\nicoreader.iss), se c'e' Inno Setup 6 (jrsoftware.org, o
#      `winget install JRSoftware.InnoSetup`); senza, lo dice e si ferma allo zip
#
# L'ingranditore AI non c'e': lo scarica chi lo vuole, al primo uso.
param([switch]$SenzaProve, [ValidateSet("", "x64", "arm64")][string]$Arch = "")
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

# senza -Arch, quella di questo PC
$qui = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }
if (-not $Arch) { $Arch = $qui }
$triple = if ($Arch -eq "arm64") { "aarch64-pc-windows-msvc" } else { "x86_64-pc-windows-msvc" }
if ($Arch -eq $qui) {
    $build = @()
    $exe = "target\release\fumetto.exe"
} else {
    $build = @("--target", $triple)
    $exe = "target\$triple\release\fumetto.exe"
}

$pdfium = "vendor\pdfium\win-$Arch"
if ($Arch -eq "arm64") {
    if (-not (Test-Path "$pdfium\bin\pdfium.dll")) {
        $versione_pdfium = (Select-String -Path "vendor\pdfium\win-x64\VERSION" -Pattern '^BUILD=(.+)').Matches[0].Groups[1].Value
        $tgz = Join-Path $env:TEMP "pdfium-win-arm64.tgz"
        Invoke-WebRequest -UseBasicParsing -OutFile $tgz `
            "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$versione_pdfium/pdfium-win-arm64.tgz"
        $impronta = (Get-FileHash $tgz -Algorithm SHA256).Hash.ToLower()
        if ($impronta -ne "5d04b6d0281e78613ef836dea2e0fefe6831f3ae92b3573e8fdf55330de67d3d") {
            Remove-Item $tgz
            throw "pdfium-win-arm64.tgz non e' il file atteso"
        }
        Remove-Item $pdfium -Recurse -Force -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Force $pdfium | Out-Null
        tar -xzf $tgz -C $pdfium LICENSE VERSION licenses bin/pdfium.dll
        if ($LASTEXITCODE -ne 0) { throw "pdfium: estrazione fallita" }
        Remove-Item $tgz
    }
}

# le prove si possono fare solo su un PC della stessa architettura
if (-not $SenzaProve -and $qui -eq $Arch) {
    cargo test --workspace --quiet
    if ($LASTEXITCODE -ne 0) { throw "le prove non passano" }
}
cargo build --release -p fumetto @build
if ($LASTEXITCODE -ne 0) { throw "compilazione fallita (exit $LASTEXITCODE)" }

$versione = (Select-String -Path "Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$dist = "dist\NicoReader"
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$dist\licenze\pdfium" | Out-Null

Copy-Item $exe "$dist\NicoReader.exe"
Copy-Item "$pdfium\bin\pdfium.dll" $dist
Copy-Item "$pdfium\LICENSE" "$dist\licenze\pdfium\LICENSE.txt"
Copy-Item "$pdfium\licenses\*" "$dist\licenze\pdfium\"
Copy-Item "crates\fumetto-render\caratteri\OFL-*.txt" "$dist\licenze\"
# la licenza di NicoReader stesso
Copy-Item "..\LICENSE-MIT" "$dist\licenze\NicoReader-LICENSE-MIT.txt"
Copy-Item "..\LICENSE-APACHE" "$dist\licenze\NicoReader-LICENSE-APACHE.txt"

# --- le licenze delle librerie Rust (e di quelle in C e C++ che contengono) ---
# Per ogni pacchetto che finisce nell'eseguibile di Windows: nome, versione,
# licenza dichiarata, e i testi di licenza che il pacchetto porta con se'.
# Le dipendenze solo per compilare o solo per le prove restano fuori.
$meta = cargo metadata --format-version 1 --filter-platform $triple | ConvertFrom-Json
$per_id = @{}
foreach ($p in $meta.packages) { $per_id[$p.id] = $p }
$nodi = @{}
foreach ($n in $meta.resolve.nodes) { $nodi[$n.id] = $n }
$radice = ($meta.packages | Where-Object { $_.name -eq "fumetto" }).id
$visti = @{}
$coda = New-Object System.Collections.Queue
$coda.Enqueue($radice)
while ($coda.Count -gt 0) {
    $id = $coda.Dequeue()
    if ($visti.ContainsKey($id)) { continue }
    $visti[$id] = $true
    foreach ($d in $nodi[$id].deps) {
        # solo le dipendenze normali: niente build.rs ne' prove
        if ($d.dep_kinds | Where-Object { $null -eq $_.kind }) { $coda.Enqueue($d.pkg) }
    }
}
$testo = New-Object System.Text.StringBuilder
[void]$testo.AppendLine("NicoReader $versione contiene queste librerie. Qui sotto le loro licenze.")
[void]$testo.AppendLine("NicoReader contains these libraries. Their licenses follow.")
[void]$testo.AppendLine("")
$pacchetti = $visti.Keys | ForEach-Object { $per_id[$_] } | Where-Object { $_.source } | Sort-Object name, version
foreach ($p in $pacchetti) {
    [void]$testo.AppendLine("=" * 78)
    [void]$testo.AppendLine("$($p.name) $($p.version)   ($($p.license))")
    if ($p.repository) { [void]$testo.AppendLine($p.repository) }
    [void]$testo.AppendLine("=" * 78)
    $cartella = Split-Path $p.manifest_path
    $file = Get-ChildItem $cartella -File -Recurse -Depth 2 |
        Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE|license)' } |
        Sort-Object FullName
    foreach ($f in $file) {
        [void]$testo.AppendLine("--- $($f.FullName.Substring($cartella.Length + 1))")
        [void]$testo.AppendLine((Get-Content $f.FullName -Raw))
    }
    if (-not $file) { [void]$testo.AppendLine("(testo non incluso nel pacchetto: vale la licenza dichiarata)") }
    [void]$testo.AppendLine("")
}
[System.IO.File]::WriteAllText("$PWD\$dist\licenze\terze-parti.txt", $testo.ToString(), (New-Object System.Text.UTF8Encoding $false))

$mb = [math]::Round(((Get-ChildItem $dist -Recurse -File | Measure-Object Length -Sum).Sum / 1MB), 1)
Write-Host ""
Write-Host "Fatto -> $dist\NicoReader.exe  ($mb MB, $($pacchetti.Count) librerie nelle licenze)" -ForegroundColor Green

$zip = "dist\NicoReader-$versione-windows-$Arch.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$dist\*" -DestinationPath $zip -CompressionLevel Optimal
$z = [math]::Round((Get-Item $zip).Length / 1MB, 1)
Write-Host "Pacchetto -> $zip  ($z MB da scaricare)" -ForegroundColor Green

# --- l'installer, con Inno Setup ---
$iscc = @(
    (Get-Command iscc -ErrorAction SilentlyContinue).Source,
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) {
    Write-Host "Installer: manca Inno Setup 6, resta solo lo zip" -ForegroundColor Yellow
    exit 0
}
& $iscc /Q "/DVersione=$versione" "/DArch=$Arch" installer\nicoreader.iss
if ($LASTEXITCODE -ne 0) { throw "installer fallito (exit $LASTEXITCODE)" }
$setup = "dist\NicoReader-$versione-windows-$Arch-setup.exe"
$i = [math]::Round((Get-Item $setup).Length / 1MB, 1)
Write-Host "Installer -> $setup  ($i MB da scaricare)" -ForegroundColor Green
