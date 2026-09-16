use std::env;
use std::fs;
use std::path::PathBuf;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

const WINDOWS_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="1.0.0.0" processorArchitecture="*" name="AMRI.VPN" type="win32" />
  <description>AMRI VPN</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let source_png = manifest_dir.join("../../assets/brand/amri-icon.png");
    println!("cargo:rerun-if-changed={}", source_png.display());

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let generated_ico = out_dir.join("amri-vpn.ico");
    wrap_png_as_ico(&source_png, &generated_ico)?;

    let icon_path = generated_ico
        .to_str()
        .ok_or("generated icon path is not valid UTF-8")?;

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon_path)
        .set_manifest(WINDOWS_MANIFEST)
        .set("ProductName", "AMRI VPN")
        .set("FileDescription", "AMRI VPN")
        .set("CompanyName", "AMRI")
        .set("InternalName", "AMRI-VPN.exe")
        .set("OriginalFilename", "AMRI-VPN.exe");
    resource.compile()?;

    Ok(())
}

fn wrap_png_as_ico(
    source_png: &std::path::Path,
    output_ico: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let png = fs::read(source_png)?;
    if png.len() < 24 || png.get(..8) != Some(PNG_SIGNATURE) || png.get(12..16) != Some(b"IHDR") {
        return Err("canonical AMRI icon is not a valid PNG".into());
    }

    let width = u32::from_be_bytes(png[16..20].try_into()?);
    let height = u32::from_be_bytes(png[20..24].try_into()?);
    if !(1..=256).contains(&width) || !(1..=256).contains(&height) {
        return Err("Windows ICO wrapper requires PNG dimensions from 1 to 256 pixels".into());
    }

    let png_len = u32::try_from(png.len())?;
    let mut ico = Vec::with_capacity(22 + png.len());
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // image type: icon
    ico.extend_from_slice(&1u16.to_le_bytes()); // one image
    ico.push(if width == 256 { 0 } else { width as u8 });
    ico.push(if height == 256 { 0 } else { height as u8 });
    ico.push(0); // palette count: PNG owns its palette
    ico.push(0); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&32u16.to_le_bytes());
    ico.extend_from_slice(&png_len.to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(&png);
    fs::write(output_ico, ico)?;
    Ok(())
}
