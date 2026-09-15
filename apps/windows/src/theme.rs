use eframe::egui::{Color32, Vec2};

// AMRI desktop theme tokens. Edit this file to restyle the app without touching VPN logic.
pub const PANEL: Color32 = Color32::from_rgb(13, 16, 22);
pub const WINDOW: Color32 = Color32::from_rgb(19, 23, 31);
pub const BACKDROP: Color32 = Color32::from_rgb(10, 12, 17);
pub const SURFACE_MUTED: Color32 = Color32::from_rgb(24, 29, 39);
pub const SURFACE: Color32 = Color32::from_rgb(22, 27, 36);
pub const HERO_SURFACE: Color32 = Color32::from_rgb(24, 31, 44);
pub const ACCENT: Color32 = Color32::from_rgb(67, 104, 255);
pub const TEXT_MUTED: Color32 = Color32::from_gray(150);
pub const CARD_RADIUS: u8 = 18;
pub const HERO_RADIUS: u8 = 24;
pub const CONTROL_RADIUS: u8 = 12;
pub const CARD_MARGIN: i8 = 18;
pub const HERO_MARGIN: i8 = 24;

pub fn item_spacing() -> Vec2 {
    Vec2::new(12.0, 12.0)
}

pub fn button_padding() -> Vec2 {
    Vec2::new(16.0, 10.0)
}
