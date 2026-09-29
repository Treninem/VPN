#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesktopLayout {
    Compact,
    Wide,
}

pub(crate) const MIN_WINDOW_WIDTH: f32 = 560.0;
pub(crate) const MIN_WINDOW_HEIGHT: f32 = 520.0;
pub(crate) const WIDE_LAYOUT_BREAKPOINT: f32 = 920.0;
pub(crate) const MAX_CONTENT_WIDTH: f32 = 1120.0;

pub(crate) fn desktop_layout(available_width: f32) -> DesktopLayout {
    if available_width >= WIDE_LAYOUT_BREAKPOINT {
        DesktopLayout::Wide
    } else {
        DesktopLayout::Compact
    }
}

pub(crate) fn content_width(available_width: f32) -> f32 {
    available_width.clamp(1.0, MAX_CONTENT_WIDTH)
}

pub(crate) fn stacks_card_actions(available_width: f32) -> bool {
    available_width < 520.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_layout_covers_minimum_and_split_screen_widths() {
        assert_eq!(desktop_layout(MIN_WINDOW_WIDTH), DesktopLayout::Compact);
        assert_eq!(desktop_layout(720.0), DesktopLayout::Compact);
        assert_eq!(
            desktop_layout(WIDE_LAYOUT_BREAKPOINT - 1.0),
            DesktopLayout::Compact
        );
    }

    #[test]
    fn wide_layout_starts_at_declared_breakpoint() {
        assert_eq!(desktop_layout(WIDE_LAYOUT_BREAKPOINT), DesktopLayout::Wide);
        assert_eq!(desktop_layout(3840.0), DesktopLayout::Wide);
    }

    #[test]
    fn content_never_exceeds_readable_maximum() {
        assert_eq!(content_width(640.0), 640.0);
        assert_eq!(content_width(3840.0), MAX_CONTENT_WIDTH);
        assert_eq!(content_width(0.0), 1.0);
    }

    #[test]
    fn narrow_cards_stack_actions() {
        assert!(stacks_card_actions(519.0));
        assert!(!stacks_card_actions(520.0));
    }
}
