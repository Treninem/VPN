use image::imageops::FilterType;
use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

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
    render_windows_icon(&source_png, &generated_ico)?;

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

fn render_windows_icon(
    source_png: &Path,
    output_ico: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let png = fs::read(source_png)?;
    let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?;
    let bounded = if decoded.width() > 256 || decoded.height() > 256 {
        decoded.resize(256, 256, FilterType::Lanczos3)
    } else {
        decoded
    };
    let rgba = bounded.to_rgba8();
    let (width, height) = rgba.dimensions();
    let icon_image = ico::IconImage::from_rgba_data(width, height, rgba.into_raw());
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    icon_dir.add_entry(ico::IconDirEntry::encode(&icon_image)?);
    icon_dir.write(File::create(output_ico)?)?;
    Ok(())
}
