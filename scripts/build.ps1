[CmdletBinding()]
param([switch]$PackageInfo)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$localTools = Join-Path $workspace '.spacesweep-dev'
$localCargo = Join-Path $localTools 'cargo\bin\cargo.exe'
$oldCargoHome = $env:CARGO_HOME
$oldRustupHome = $env:RUSTUP_HOME
$oldPath = $env:Path
$oldOutputEncoding = [Console]::OutputEncoding

Push-Location -LiteralPath $workspace
try {
    if (Test-Path -LiteralPath $localCargo -PathType Leaf) {
        $cargo = $localCargo
        $env:CARGO_HOME = Join-Path $localTools 'cargo'
        $env:RUSTUP_HOME = Join-Path $localTools 'rustup'
        $binutils = Join-Path $localTools 'binutils\mingw64\bin'
        if (Test-Path -LiteralPath $binutils -PathType Container) {
            $env:Path = $binutils + [IO.Path]::PathSeparator + $env:Path
        }
    }
    else {
        $cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop).Source
    }

    $buildArgs = @('build', '--release', '--locked')
    if ($PackageInfo) {
        [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
        $messages = & $cargo @buildArgs --message-format=json-render-diagnostics
    }
    else {
        & $cargo @buildArgs
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build failed with exit code $LASTEXITCODE."
    }

    if ($PackageInfo) {
        $artifacts = @($messages | ConvertFrom-Json | Where-Object reason -eq 'compiler-artifact')
        $engine = $artifacts | Where-Object {
            $_.target.name -eq 'spacesweep' -and $_.executable
        } | Select-Object -ExpandProperty executable
        $dependencies = $artifacts | Where-Object { $_.manifest_path -ne (Join-Path $workspace 'Cargo.toml') } |
            ForEach-Object { Split-Path -Parent $_.manifest_path } | Sort-Object -Unique
        $rustc = if ($env:RUSTC) { $env:RUSTC } else { Join-Path (Split-Path -Parent $cargo) 'rustc.exe' }
        $sysroot = & $rustc --print sysroot
        if ($LASTEXITCODE -ne 0) {
            throw "Could not locate the Rust standard library notices (exit $LASTEXITCODE)."
        }
        [pscustomobject]@{
            Engine = $engine
            Dependencies = $dependencies
            RustDocDir = Join-Path $sysroot 'share\doc\rust'
        }
    }
}
finally {
    Pop-Location
    $env:CARGO_HOME = $oldCargoHome
    $env:RUSTUP_HOME = $oldRustupHome
    $env:Path = $oldPath
    if ($PackageInfo) { [Console]::OutputEncoding = $oldOutputEncoding }
}
