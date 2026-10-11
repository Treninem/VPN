param(
    [Parameter(Mandatory = $true)]
    [string]$PfxPath,

    [Parameter(Mandatory = $true)]
    [string]$PfxPassword,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedThumbprint,

    [Parameter(Mandatory = $true)]
    [string[]]$Files,

    [string]$TimestampUrl = "",

    [bool]$AllowUntrustedChain = $false
)

$ErrorActionPreference = "Stop"

function Normalize-Thumbprint([string]$Value) {
    return ($Value -replace '[^0-9A-Fa-f]', '').ToUpperInvariant()
}

function Find-SignTool {
    $roots = @(
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin",
        "${env:ProgramFiles}\Windows Kits\10\bin"
    ) | Where-Object { Test-Path -LiteralPath $_ }

    $candidate = @(
        foreach ($root in $roots) {
            Get-ChildItem -LiteralPath $root -Recurse -Filter signtool.exe -File -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match '[\\/]x64[\\/]signtool\.exe$' }
        }
    ) | Sort-Object FullName -Descending | Select-Object -First 1

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
$existingThumbprints = @(Get-ChildItem "Cert:\CurrentUser\My" | ForEach-Object Thumbprint)
$importedThumbprints = @()

try {
    $imported = @(Import-PfxCertificate -FilePath $PfxPath -CertStoreLocation "Cert:\CurrentUser\My" -Password $securePassword -Exportable:$false)
    $importedThumbprints = @($imported | ForEach-Object Thumbprint)
    $certificate = $imported | Where-Object { $_.HasPrivateKey } | Select-Object -First 1
    if (-not $certificate) {
        throw "Windows signing certificate with a private key was not imported"
    }
    if ((Normalize-Thumbprint $certificate.Thumbprint) -ne $expected) {
        throw "Windows signing certificate thumbprint mismatch"
    }
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

        $signature = Get-AuthenticodeSignature -LiteralPath $file
        if (-not $signature.SignerCertificate) {
            throw "No Authenticode signer certificate was found on $file"
        }
        if ((Normalize-Thumbprint $signature.SignerCertificate.Thumbprint) -ne $expected) {
            throw "Authenticode signer thumbprint mismatch for $file"
        }

        if ($AllowUntrustedChain) {
            # PR certificates are self-signed. Windows may report UnknownError for their
            # trust chain even when the embedded signer thumbprint matches. Production
            # releases always use the strict signtool and Valid checks below.
            if ($signature.Status -notin @(
                [System.Management.Automation.SignatureStatus]::Valid,
                [System.Management.Automation.SignatureStatus]::NotTrusted,
                [System.Management.Automation.SignatureStatus]::UnknownError
            )) {
                throw "Authenticode signature is invalid for $file ($($signature.Status))"
            }
        }
        else {
            & $signtool verify /pa /v $file
            if ($LASTEXITCODE -ne 0 -or $signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
                throw "Authenticode trust verification failed for $file ($($signature.Status))"
            }
        }
    }
}
finally {
    foreach ($thumbprint in $importedThumbprints) {
        if ($thumbprint -and $thumbprint -notin $existingThumbprints) {
            $certPath = "Cert:\CurrentUser\My\$thumbprint"
            if (Test-Path -LiteralPath $certPath) {
                Remove-Item -LiteralPath $certPath -Force -ErrorAction SilentlyContinue
            }
        }
    }
}
