param(
    [Parameter(Mandatory = $true)]
    [string]$PfxPath,

    [Parameter(Mandatory = $true)]
    [string]$PfxPassword,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedThumbprint,

    [Parameter(Mandatory = $true)]
    [string[]]$Files,

    [string]$TimestampUrl = ""
)

$ErrorActionPreference = "Stop"

function Normalize-Thumbprint([string]$Value) {
    return ($Value -replace '[^0-9A-Fa-f]', '').ToUpperInvariant()
}

function Find-SignTool {
    $roots = @(
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin",
        "${env:ProgramFiles}\Windows Kits\10\bin"
    ) | Where-Object { Test-Path $_ }

    $candidate = foreach ($root in $roots) {
        Get-ChildItem -Path $root -Recurse -Filter signtool.exe -File -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '[\\/]x64[\\/]signtool\.exe$' }
    } | Sort-Object FullName -Descending | Select-Object -First 1

    if (-not $candidate) {
        throw "signtool.exe was not found in the installed Windows SDK"
    }
    return $candidate.FullName
}

if (-not (Test-Path -LiteralPath $PfxPath)) {
    throw "Windows signing PFX was not found"
}

$expected = Normalize-Thumbprint $ExpectedThumbprint
if ([string]::IsNullOrWhiteSpace($expected)) {
    throw "Expected Windows certificate thumbprint is empty"
}

foreach ($file in $Files) {
    if (-not (Test-Path -LiteralPath $file)) {
        throw "Signing target was not found: $file"
    }
}

$securePassword = ConvertTo-SecureString $PfxPassword -AsPlainText -Force
$certificate = $null
$certPath = $null

try {
    $certificate = Import-PfxCertificate -FilePath $PfxPath -CertStoreLocation "Cert:\CurrentUser\My" -Password $securePassword -Exportable:$false

    if (-not $certificate) {
        throw "Windows signing certificate import failed"
    }

    $actual = Normalize-Thumbprint $certificate.Thumbprint
    if ($actual -ne $expected) {
        throw "Windows signing certificate thumbprint mismatch"
    }

    if (-not $certificate.HasPrivateKey) {
        throw "Windows signing certificate does not contain a private key"
    }

    $certPath = "Cert:\CurrentUser\My\$($certificate.Thumbprint)"
    $signtool = Find-SignTool

    foreach ($file in $Files) {
        $signArgs = @("sign", "/sha1", $certificate.Thumbprint, "/fd", "SHA256")
        if (-not [string]::IsNullOrWhiteSpace($TimestampUrl)) {
            $signArgs += @("/tr", $TimestampUrl, "/td", "SHA256")
        }
        $signArgs += $file

        & $signtool @signArgs
        if ($LASTEXITCODE -ne 0) {
            throw "signtool failed while signing $file"
        }

        & $signtool verify /pa /v $file
        if ($LASTEXITCODE -ne 0) {
            throw "signtool verification failed for $file"
        }

        $signature = Get-AuthenticodeSignature -LiteralPath $file
        if (-not $signature.SignerCertificate) {
            throw "No Authenticode signer certificate was found on $file"
        }
        $signedBy = Normalize-Thumbprint $signature.SignerCertificate.Thumbprint
        if ($signedBy -ne $expected) {
            throw "Authenticode signer thumbprint mismatch for $file"
        }
        if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
            throw "Authenticode signature is not valid for $file ($($signature.Status))"
        }
    }
}
finally {
    if ($certPath -and (Test-Path $certPath)) {
        Remove-Item -LiteralPath $certPath -Force -ErrorAction SilentlyContinue
    }
}
