$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try {
    cargo build --locked --release --features ui
    if ($LASTEXITCODE -ne 0) { throw 'Native build failed' }
    $packageName = 'httpied-windows-x86_64'
    $stage = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
    try {
        $package = Join-Path $stage $packageName
        New-Item -ItemType Directory -Force -Path @($package, 'dist') | Out-Null
        Copy-Item 'target/release/httpied.exe', 'README.md' $package
        New-Item -ItemType Directory -Force -Path (Join-Path $package '.github/workflows') | Out-Null
        Copy-Item '.github/workflows/check.yml' (Join-Path $package '.github/workflows')
        $archive = Join-Path 'dist' "$packageName.zip"
        Compress-Archive -Path $package -DestinationPath $archive -Force
        (Get-FileHash -Algorithm SHA256 $archive).Hash | Set-Content "$archive.sha256"
    } finally { if (Test-Path $stage) { Remove-Item -Recurse -Force $stage } }
} finally { Pop-Location }
