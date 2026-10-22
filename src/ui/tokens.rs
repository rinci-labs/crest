//! Design tokens: one source for spacing, radii, type sizes, opacity, and motion.
//!
//! These numbers used to be scattered as literals across `mod.rs` and `pages.rs`, so even
//! similar pages could end up with different spacing, radii, or highlight strength. Every
//! page now takes its values from here.

/// Content inset from the left/right edge of the shape: the deck's inset plus 16 DIP of padding
/// inside it, the same padding the pages keep above and below their content.
pub const INSET: f32 = 26.0;
/// Gap between columns on Home.
pub const GUTTER: f32 = 20.0;
/// Gap between adjacent elements (e.g. buttons in one row).
pub const TIGHT: f32 = 10.0;
/// The open panel's height (normal and wide); Settings uses a taller one.
pub const PANEL_H: f32 = 192.0;
/// The page deck below the floating navigation rail: inset by the same 10 DIP from the sides
/// and the bottom edge of the island, so its corners are concentric with the island's (see
/// `RADIUS_DECK`). Top and bottom are page coordinates, like every page number.
pub const DECK_INSET: f32 = 10.0;
pub const DECK_TOP: f32 = 28.0;
pub const DECK_BOTTOM: f32 = PANEL_H - CONTENT_SHIFT - DECK_INSET;

/// Center line of the welcome title (the welcome has no tab row).
pub const HEADER_Y: f32 = 22.0;
/// Page content sits this far below its own coordinates, under the tab row; the renderer
/// applies it as a transform (and to click areas), so pages keep their own simple numbers.
pub const CONTENT_SHIFT: f32 = 14.0;
/// Center line of a page's main element: the Focus/Timer ring and clock, the Water ring, the
/// mascot on Home. Keeps the big element of every page on one line.
pub const HERO_Y: f32 = 77.0;
/// Button row at the bottom of a page: its bottom edge keeps 16 DIP to the deck's bottom.
pub const ROW_Y: f32 = DECK_BOTTOM - 16.0 - CONTROL_H / 2.0;
/// Center line of the tab row at the top of the panel (tabs on the left, the page's caption
/// on the right).
pub const TABS_Y: f32 = 20.0;
/// Width of one tab-bar icon slot, and the height of its key (current or hovered page).
pub const TAB_SLOT: f32 = 24.0;
pub const TAB_H: f32 = 18.0;
/// Height of a task row and an agent row.
pub const ROW_H: f32 = 24.0;
/// Height of a calendar agenda row (tighter since it sits beside the date).
pub const AGENDA_ROW_H: f32 = 16.0;
/// Height of input boxes and pill buttons in the bottom row.
pub const CONTROL_H: f32 = 24.0;
/// Shelf tile height.
pub const TILE_H: f32 = 80.0;
/// Distance between shelf tiles.
pub const TILE_PITCH: f32 = 76.0;

/// Corner of small keys (Caps Lock indicator, checkbox).
pub const RADIUS_KEY: f32 = 4.0;
/// Corner of list rows, tiles, and input boxes.
pub const RADIUS_ROW: f32 = 10.0;
/// Corner of large cards (file drop area, news panel).
pub const RADIUS_CARD: f32 = 14.0;
/// Bottom corner of the open island.
pub const RADIUS_ISLAND: f32 = 28.0;
/// The page deck's corner: the island's minus the deck's inset, so the two curves are concentric.
pub const RADIUS_DECK: f32 = RADIUS_ISLAND - DECK_INSET;

/// Background of an idle button.
pub const FILL_SUBTLE: f32 = 0.08;
/// Resting fill for nested surfaces and list rows.
pub const FILL_GHOST: f32 = 0.045;
/// Background of a hovered button.
pub const FILL_SOFT: f32 = 0.12;
/// Background of the active choice.
pub const FILL_STRONG: f32 = 0.16;
/// Track of progress bars and rings.
pub const TRACK: f32 = 0.22;
/// Column divider line.
pub const DIVIDER: f32 = 0.12;
/// Hairline around nested surfaces and the outside of the island.
pub const HAIRLINE: f32 = 0.10;

/// Type sizes (DIP). A small scale, but nothing below 11 DIP so captions stay readable at
/// 100% display scale.
pub const TYPE_TINY: f32 = 11.0;
pub const TYPE_CAPTION: f32 = 12.0;
pub const TYPE_BODY: f32 = 13.0;
pub const TYPE_LABEL: f32 = 14.0;
pub const TYPE_TITLE: f32 = 17.0;
/// Lyrics use a larger display line and a quieter surrounding line.
pub const TYPE_LYRIC_ACTIVE: f32 = 16.0;
pub const TYPE_LYRIC_CONTEXT: f32 = 13.0;

/// Slide distance of the content when switching pages.
pub const PAGE_SLIDE: f32 = 28.0;
/// Slide distance of Home's right column when switching between calendar and lyrics.
pub const COLUMN_SLIDE: f32 = 36.0;
/// Lyrics column, as Apple Music shows it: the line being sung (room for two lines) and the next
/// one below it, the pair centred in the card. `LYRIC_CENTER` is the sung line's centre and
/// `LYRIC_GAP` the step to the next line; the previous line fades out above.
pub const LYRIC_GAP: f32 = 44.0;
pub const LYRIC_CENTER: f32 = (DECK_TOP + DECK_BOTTOM) / 2.0 - LYRIC_GAP / 2.0 + 6.0;

/// Width of Home's left column next to the calendar, as a share of the content width.
pub const HOME_LEFT_RATIO: f32 = 0.545;
/// Width of the player column while the lyrics column shows, as a share of the content width.
pub const LYRIC_LEFT_RATIO: f32 = 0.46;
/// Lower and upper bounds of the left column, so it stays sensible on narrow and wide screens.
pub const HOME_LEFT_MIN: f32 = 264.0;
pub const HOME_LEFT_MAX: f32 = 380.0;
pub const LYRIC_LEFT_MIN: f32 = 236.0;
pub const LYRIC_LEFT_MAX: f32 = 320.0;

/// The widest shape may only use this share of the monitor width, so the panel does not
/// cover an entire narrow screen.
pub const MAX_SCREEN_SHARE: f32 = 0.94;

/// Width of Home's left column for content width `inner`.
pub fn home_left(inner: f32) -> f32 {
    (inner * HOME_LEFT_RATIO).clamp(HOME_LEFT_MIN, HOME_LEFT_MAX).min(inner * 0.7)
}

/// Width of the player column while the lyrics column shows, for content width `inner`.
pub fn lyric_left(inner: f32) -> f32 {
    (inner * LYRIC_LEFT_RATIO).clamp(LYRIC_LEFT_MIN, LYRIC_LEFT_MAX).min(inner * 0.6)
}

/// The bottom row ends inside the deck, and the deck inside the panel.
const _: () = assert!(ROW_Y + CONTROL_H / 2.0 < DECK_BOTTOM);
const _: () = assert!(DECK_BOTTOM + CONTENT_SHIFT + DECK_INSET == PANEL_H);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_always_fit_the_row() {
        for inner in [200.0, 320.0, 384.0, 604.0, 900.0] {
            assert!(home_left(inner) <= inner * 0.7, "left column fits width {inner}");
            assert!(lyric_left(inner) <= inner * 0.6, "player column fits width {inner}");
        }
    }

    #[test]
    fn columns_grow_with_the_panel() {
        assert!(home_left(700.0) > home_left(420.0), "left column widens");
        assert_eq!(home_left(300.0), HOME_LEFT_MIN.min(300.0 * 0.7), "clamped to the minimum");
        assert_eq!(home_left(2000.0), HOME_LEFT_MAX, "clamped to the maximum");
    }
}
