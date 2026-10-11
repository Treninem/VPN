# AMRI VPN production signing

Production signing secrets are owner-controlled and must never be committed to Git.

## Windows Authenticode

The release workflow requires these GitHub Secrets for non-PR release runs:

- `AMRI_WINDOWS_PFX_BASE64` — Base64 of the owner code-signing PFX.
- `AMRI_WINDOWS_PFX_PASSWORD` — PFX password.
- `AMRI_WINDOWS_CERT_THUMBPRINT` — expected signer certificate thumbprint.

The workflow:

1. materializes the PFX only inside the ephemeral Windows runner;
2. verifies the imported certificate thumbprint before signing;
3. signs `AMRI-VPN.exe` and `AMRI-VPN-Launcher.exe` before building the portable ZIP;
4. builds the NSIS installer;
5. signs `AMRI-VPN-Windows-Setup.exe`;
6. verifies every Authenticode signature with `signtool verify /pa` and PowerShell;
7. uses SHA-256 file digests and RFC3161 timestamping for production release runs;
8. removes the imported signing certificate from the runner certificate store after each signing step.

The PFX password is not passed on the `signtool` command line. Pull requests exercise the same signing path with a short-lived self-signed CI certificate so production secrets are never exposed to PR jobs.

## Android

The release workflow already requires:

- `AMRI_ANDROID_KEYSTORE_BASE64`
- `AMRI_ANDROID_KEYSTORE_PASSWORD`
- `AMRI_ANDROID_KEY_ALIAS`
- `AMRI_ANDROID_KEY_PASSWORD`
- `AMRI_ANDROID_CERT_SHA256`

Release APK signing is verified before publication. Pull requests use an ephemeral test keystore.

## Release branches

Production prerelease publication is triggered from `release/v*` or explicit workflow dispatch. Non-PR release runs fail closed when owner signing secrets are absent or when certificate fingerprints/thumbprints do not match.

Installable CI artifacts produced by ordinary `main`/PR package workflows are test artifacts. They must not be described as owner-production-signed unless they came through the production signing release workflow with owner secrets.

## Preparing secret values

Windows PFX Base64 can be produced locally without uploading the raw certificate anywhere else:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("AMRI-CodeSigning.pfx")) | Set-Clipboard
```

Certificate thumbprint:

```powershell
$cert = Get-PfxCertificate "AMRI-CodeSigning.pfx"
$cert.Thumbprint
```

Android keystore Base64:

```bash
base64 -w 0 amri-production-release.jks
```

Keep original signing material backed up offline. Losing the Android production key prevents seamless application updates signed as the same app identity.
