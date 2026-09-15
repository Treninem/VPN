use eframe::egui::{Color32, Vec2};

include!("theme_generated.rs");

pub fn item_spacing() -> Vec2 {
    Vec2::new(ITEM_SPACING_X, ITEM_SPACING_Y)
}

pub fn button_padding() -> Vec2 {
    Vec2::new(BUTTON_PADDING_X, BUTTON_PADDING_Y)
}
