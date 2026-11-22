[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$localTools = Join-Path $workspace '.spacesweep-dev'
$localCargo = Join-Path $localTools 'cargo\bin\cargo.exe'
$oldCargoHome = $env:CARGO_HOME
$oldRustupHome = $env:RUSTUP_HOME
$oldPath = $env:Path

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

    & $cargo build --release --locked
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
    $env:CARGO_HOME = $oldCargoHome
    $env:RUSTUP_HOME = $oldRustupHome
    $env:Path = $oldPath
}
