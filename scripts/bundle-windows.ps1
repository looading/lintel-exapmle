[CmdletBinding()]
param(
    [string]$OutputZip = ""
)

$ErrorActionPreference = 'Stop'
$project = Split-Path -Parent $PSScriptRoot
$target = 'x86_64-pc-windows-msvc'
$packageName = 'world-clock-windows-x64'
$previousRustFlags = $env:RUSTFLAGS
$previousEncodedRustFlags = $env:CARGO_ENCODED_RUSTFLAGS
$staging = $null

if ($OutputZip) {
    $OutputZip = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputZip)
    if ([IO.Path]::GetExtension($OutputZip) -ne '.zip') {
        throw 'OutputZip must have a .zip extension.'
    }
}

Push-Location -LiteralPath $project
try {
    # Keep the Microsoft C runtime inside the portable executable.
    if ($null -ne $previousEncodedRustFlags) {
        $env:CARGO_ENCODED_RUSTFLAGS = (@($previousEncodedRustFlags, '-Ctarget-feature=+crt-static') |
            Where-Object { $_ }) -join [char]0x1f
    } else {
        $env:RUSTFLAGS = "$previousRustFlags -Ctarget-feature=+crt-static".Trim()
    }

    $metadataJson = cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE."
    }
    $metadata = $metadataJson | ConvertFrom-Json
    $targetDirectory = $metadata.target_directory

    cargo build --release --locked --target $target --bin lintel-world-clock
    if ($LASTEXITCODE -ne 0) {
        throw "Windows release build failed with exit code $LASTEXITCODE."
    }

    if (!$OutputZip) {
        $OutputZip = Join-Path (Join-Path $targetDirectory 'windows') "$packageName.zip"
    }
    $outputDirectory = Split-Path -Parent $OutputZip
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
    $staging = Join-Path $outputDirectory ('.staging-' + [guid]::NewGuid().ToString('N'))
    $packageDirectory = Join-Path $staging $packageName
    New-Item -ItemType Directory -Path $packageDirectory | Out-Null

    $executable = Join-Path (Join-Path (Join-Path $targetDirectory $target) 'release') 'lintel-world-clock.exe'
    Copy-Item -LiteralPath $executable -Destination (Join-Path $packageDirectory 'world-clock.exe')
    $stagedZip = Join-Path $staging "$packageName.zip"
    Compress-Archive -LiteralPath $packageDirectory -DestinationPath $stagedZip -CompressionLevel Optimal
    if (Test-Path -LiteralPath $OutputZip -PathType Leaf) {
        [IO.File]::Replace($stagedZip, $OutputZip, $null)
    } else {
        [IO.File]::Move($stagedZip, $OutputZip)
    }
    Write-Output $OutputZip
} finally {
    $env:RUSTFLAGS = $previousRustFlags
    $env:CARGO_ENCODED_RUSTFLAGS = $previousEncodedRustFlags
    if ($staging -and (Test-Path -LiteralPath $staging)) {
        Remove-Item -LiteralPath $staging -Recurse -Force
    }
    Pop-Location
}
