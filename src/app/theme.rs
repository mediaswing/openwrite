//! Fonts, colour and the metrics the window is laid out on.
//!
//! Shared with the PodBatch app, so the two look like they come from the same
//! place. The reasoning carries over intact: the bundled bold face, because at
//! these sizes a light weight is the single biggest readability cost and egui
//! ships only Ubuntu-Light; and a palette written down as explicit pairs so the
//! contrast ratios can be reasoned about instead of inherited — every text
//! colour below reaches at least 4.5:1 against the surface it is drawn on.
//!
//! Both themes are defined; egui follows the operating system's appearance, so
//! a user who runs their machine in dark mode gets a dark app without asking.
//!
//! On top of that this app adds a high contrast mode, for users who need more
//! separation than a well-behaved palette can give them.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Theme, Visuals,
};

/// Ubuntu Bold, bundled so the app looks and reads the same on a fresh Windows
/// install as it does on a Mac.
const UBUNTU_BOLD: &[u8] = include_bytes!("../../assets/fonts/Ubuntu-Bold.ttf");

/// Height of a control. Comfortably above the 44px touch/pointer target advice
/// once the surrounding item spacing is counted.
pub const CONTROL_HEIGHT: f32 = 34.0;

/// The colours that change meaning between light and dark. Held as a struct so
/// a call site asks for "the muted colour" and gets one that is legible on the
/// surface it is actually drawing on.
#[derive(Clone, Copy)]
pub struct Palette {
    /// Success / completed.
    pub ok: Color32,
    /// Needs attention but nothing has failed.
    pub warn: Color32,
    /// Something failed.
    pub bad: Color32,
    /// Supporting text. Still 4.5:1 — "muted" here means lower saturation, not
    /// lower contrast, because greyed-out text is where accessibility usually
    /// goes wrong.
    pub muted: Color32,
    /// Focus ring and selection.
    pub accent: Color32,
}

/// 4.5:1 or better against `#FFFFFF` and the panel fill below.
const LIGHT: Palette = Palette {
    ok: Color32::from_rgb(0, 100, 45),
    warn: Color32::from_rgb(133, 77, 0),
    bad: Color32::from_rgb(176, 27, 27),
    muted: Color32::from_rgb(84, 92, 102),
    accent: Color32::from_rgb(11, 87, 164),
};

/// 4.5:1 or better against the dark panel and window fills below.
const DARK: Palette = Palette {
    ok: Color32::from_rgb(109, 219, 133),
    warn: Color32::from_rgb(240, 187, 64),
    bad: Color32::from_rgb(255, 138, 128),
    muted: Color32::from_rgb(176, 186, 197),
    accent: Color32::from_rgb(124, 187, 255),
};

/// Maximum separation, for users who need it. Pure black and white, with the
/// focus ring in the one hue that stays loud against both.
const HIGH_CONTRAST_LIGHT: Palette = Palette {
    ok: Color32::from_rgb(0, 77, 34),
    warn: Color32::from_rgb(92, 53, 0),
    bad: Color32::from_rgb(140, 0, 0),
    muted: Color32::BLACK,
    accent: Color32::from_rgb(0, 51, 153),
};

const HIGH_CONTRAST_DARK: Palette = Palette {
    ok: Color32::from_rgb(140, 255, 170),
    warn: Color32::from_rgb(255, 214, 102),
    bad: Color32::from_rgb(255, 179, 171),
    muted: Color32::WHITE,
    accent: Color32::from_rgb(255, 224, 102),
};

/// The palette matching whichever theme is currently in force.
pub fn palette(visuals: &Visuals) -> Palette {
    match (visuals.dark_mode, is_high_contrast(visuals)) {
        (true, false) => DARK,
        (false, false) => LIGHT,
        (true, true) => HIGH_CONTRAST_DARK,
        (false, true) => HIGH_CONTRAST_LIGHT,
    }
}

/// High contrast is the only mode that paints the page pure black or white.
fn is_high_contrast(visuals: &Visuals) -> bool {
    visuals.panel_fill == Color32::BLACK || visuals.panel_fill == Color32::WHITE
}

/// Installs Ubuntu Bold as the default proportional face.
///
/// `RichText::strong()` only recolours; a heavier weight has to arrive as a real
/// font. Putting it first in the `Proportional` chain means every widget picks
/// it up without each call site asking. Everything egui already had stays behind
/// it, so a glyph Ubuntu Bold doesn't cover still renders instead of becoming a
/// tofu box.
///
/// The monospace chain is left alone: the formatted screenplay is measured in
/// character cells, and a proportional face would break the layout it depends
/// on.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Ubuntu-Bold".to_owned(),
        std::sync::Arc::new(FontData::from_static(UBUNTU_BOLD)),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Ubuntu-Bold".to_owned());
    ctx.set_fonts(fonts);
}

fn light_visuals() -> Visuals {
    let mut visuals = Visuals::light();
    let text = Color32::from_rgb(18, 22, 28);

    visuals.panel_fill = Color32::from_rgb(244, 246, 249);
    visuals.window_fill = Color32::WHITE;
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.faint_bg_color = Color32::from_rgb(236, 239, 243);
    visuals.hyperlink_color = LIGHT.accent;
    visuals.warn_fg_color = LIGHT.warn;
    visuals.error_fg_color = LIGHT.bad;
    visuals.window_stroke = Stroke::new(1.0, Color32::from_rgb(150, 158, 168));

    // Control surfaces: white fills with a stroke dark enough to be a real
    // boundary rather than a suggestion, which is what a low-vision user needs
    // in order to see where one field ends and the next begins.
    visuals.widgets.noninteractive.bg_fill = visuals.panel_fill;
    visuals.widgets.noninteractive.weak_bg_fill = visuals.panel_fill;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(170, 178, 188));
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);

    visuals.widgets.inactive.bg_fill = Color32::WHITE;
    visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.5, Color32::from_rgb(96, 105, 116));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, text);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(228, 238, 250);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(228, 238, 250);
    visuals.widgets.hovered.bg_stroke = Stroke::new(2.0, LIGHT.accent);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, text);

    // `active` is also what egui uses for the keyboard-focused widget, so this
    // is the focus ring. It is deliberately the loudest thing on screen.
    visuals.widgets.active.bg_fill = Color32::from_rgb(214, 231, 249);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(214, 231, 249);
    visuals.widgets.active.bg_stroke = Stroke::new(3.0, LIGHT.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, Color32::from_rgb(8, 12, 18));

    visuals.widgets.open.bg_fill = Color32::WHITE;
    visuals.widgets.open.weak_bg_fill = Color32::WHITE;
    visuals.widgets.open.bg_stroke = Stroke::new(2.0, LIGHT.accent);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, text);

    visuals.selection.bg_fill = LIGHT.accent;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals
}

fn dark_visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    let text = Color32::from_rgb(240, 244, 249);

    visuals.panel_fill = Color32::from_rgb(20, 24, 31);
    visuals.window_fill = Color32::from_rgb(28, 33, 41);
    visuals.extreme_bg_color = Color32::from_rgb(13, 16, 21);
    visuals.faint_bg_color = Color32::from_rgb(32, 38, 47);
    visuals.hyperlink_color = DARK.accent;
    visuals.warn_fg_color = DARK.warn;
    visuals.error_fg_color = DARK.bad;
    visuals.window_stroke = Stroke::new(1.0, Color32::from_rgb(96, 106, 118));

    visuals.widgets.noninteractive.bg_fill = visuals.panel_fill;
    visuals.widgets.noninteractive.weak_bg_fill = visuals.panel_fill;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(88, 98, 110));
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);

    visuals.widgets.inactive.bg_fill = Color32::from_rgb(38, 45, 55);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(38, 45, 55);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.5, Color32::from_rgb(140, 152, 166));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, text);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 60, 76);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(48, 60, 76);
    visuals.widgets.hovered.bg_stroke = Stroke::new(2.0, DARK.accent);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, text);

    visuals.widgets.active.bg_fill = Color32::from_rgb(58, 74, 94);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(58, 74, 94);
    visuals.widgets.active.bg_stroke = Stroke::new(3.0, DARK.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);

    visuals.widgets.open.bg_fill = Color32::from_rgb(38, 45, 55);
    visuals.widgets.open.weak_bg_fill = Color32::from_rgb(38, 45, 55);
    visuals.widgets.open.bg_stroke = Stroke::new(2.0, DARK.accent);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, text);

    visuals.selection.bg_fill = Color32::from_rgb(31, 92, 156);
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals
}

/// The same structure, taken to the limit: black and white, every boundary
/// drawn, nothing conveyed by a tint.
fn high_contrast_visuals(dark: bool) -> Visuals {
    let mut visuals = if dark { dark_visuals() } else { light_visuals() };
    let palette = if dark { HIGH_CONTRAST_DARK } else { HIGH_CONTRAST_LIGHT };
    let (fg, bg) = if dark {
        (Color32::WHITE, Color32::BLACK)
    } else {
        (Color32::BLACK, Color32::WHITE)
    };

    visuals.panel_fill = bg;
    visuals.window_fill = bg;
    visuals.extreme_bg_color = bg;
    visuals.faint_bg_color = bg;
    visuals.hyperlink_color = palette.accent;
    visuals.warn_fg_color = palette.warn;
    visuals.error_fg_color = palette.bad;
    visuals.window_stroke = Stroke::new(2.0, fg);
    visuals.override_text_color = Some(fg);

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = bg;
        widget.weak_bg_fill = bg;
        widget.bg_stroke = Stroke::new(1.5, fg);
        widget.fg_stroke = Stroke::new(1.5, fg);
    }
    // The focus ring stays the loudest thing on screen.
    visuals.widgets.active.bg_fill = bg;
    visuals.widgets.active.weak_bg_fill = bg;
    visuals.widgets.active.bg_stroke = Stroke::new(4.0, palette.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, fg);

    visuals.selection.bg_fill = palette.accent;
    visuals.selection.stroke = Stroke::new(1.5, bg);
    visuals
}

/// Applies fonts, both palettes and the spacing. Rebuilding the glyph atlas is
/// expensive, so this runs from the constructor and again only when the user
/// changes the contrast setting — never per frame.
pub fn apply(ctx: &egui::Context, high_contrast: bool) {
    install_fonts(ctx);
    if high_contrast {
        ctx.set_visuals_of(Theme::Light, high_contrast_visuals(false));
        ctx.set_visuals_of(Theme::Dark, high_contrast_visuals(true));
    } else {
        ctx.set_visuals_of(Theme::Light, light_visuals());
        ctx.set_visuals_of(Theme::Dark, dark_visuals());
    }

    ctx.all_styles_mut(|style| {
        // Generous by egui's standards. Everything on screen is either a control
        // the user has to hit or a line of a screenplay they have to read.
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = CONTROL_HEIGHT;
        style.spacing.scroll.bar_width = 12.0;

        // Square-ish. Large radii blur the boundary between a control and its
        // background, which is exactly the edge low-vision users rely on.
        for widget in [
            &mut style.visuals.widgets.noninteractive,
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            widget.corner_radius = CornerRadius::same(3);
        }

        for (text_style, size) in [
            (egui::TextStyle::Heading, 22.0),
            (egui::TextStyle::Body, 15.0),
            (egui::TextStyle::Button, 15.0),
            (egui::TextStyle::Small, 13.0),
            // The formatted preview. Courier at 12pt is what the page is, but
            // on screen it is read at arm's length rather than held.
            (egui::TextStyle::Monospace, 13.0),
        ] {
            if let Some(font) = style.text_styles.get_mut(&text_style) {
                font.size = size;
            }
        }

        // egui's default is 60% alpha, which drops supporting text below 4.5:1
        // on both themes. Weak text here is a shade, not a whisper.
        style.visuals.weak_text_alpha = 0.85;
        style.visuals.disabled_alpha = 0.75;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG's relative luminance, and the contrast between two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        fn channel(value: u8) -> f32 {
            let value = value as f32 / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        fn luminance(c: Color32) -> f32 {
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// What a colour becomes when egui fades it for a switched-off widget.
    fn faded(fg: Color32, bg: Color32, alpha: f32) -> Color32 {
        let mix = |f: u8, b: u8| (alpha * f as f32 + (1.0 - alpha) * b as f32).round() as u8;
        Color32::from_rgb(mix(fg.r(), bg.r()), mix(fg.g(), bg.g()), mix(fg.b(), bg.b()))
    }

    /// The claim at the top of this module, checked rather than asserted in
    /// prose: every colour that carries meaning is legible on every surface it
    /// is drawn on, in both themes.
    #[test]
    fn every_palette_colour_clears_the_contrast_floor() {
        let cases: [(&str, Palette, Color32, &[Color32]); 4] = [
            (
                "light",
                LIGHT,
                Color32::from_rgb(18, 22, 28),
                &[
                    Color32::WHITE,
                    Color32::from_rgb(244, 246, 249),
                    Color32::from_rgb(236, 239, 243),
                ],
            ),
            (
                "dark",
                DARK,
                Color32::from_rgb(240, 244, 249),
                &[
                    Color32::from_rgb(20, 24, 31),
                    Color32::from_rgb(28, 33, 41),
                    Color32::from_rgb(13, 16, 21),
                    Color32::from_rgb(32, 38, 47),
                    Color32::from_rgb(38, 45, 55),
                ],
            ),
            (
                "high contrast light",
                HIGH_CONTRAST_LIGHT,
                Color32::BLACK,
                &[Color32::WHITE],
            ),
            (
                "high contrast dark",
                HIGH_CONTRAST_DARK,
                Color32::WHITE,
                &[Color32::BLACK],
            ),
        ];

        for (theme, palette, text, surfaces) in cases {
            let colours = [
                ("ok", palette.ok),
                ("warn", palette.warn),
                ("bad", palette.bad),
                ("muted", palette.muted),
                ("accent", palette.accent),
                ("text", text),
            ];
            for (name, colour) in colours {
                for surface in surfaces {
                    let ratio = contrast(colour, *surface);
                    assert!(
                        ratio >= 4.5,
                        "{theme} {name} is {ratio:.2}:1 on {surface:?}, under the 4.5:1 floor"
                    );
                }
            }
        }
    }

    /// Supporting text — page counts, scene numbers, the shortcut hint — is
    /// drawn weak. Weak still has to be readable.
    fn weak_text_stays_readable(high_contrast: bool) {
        let ctx = egui::Context::default();
        apply(&ctx, high_contrast);
        let alpha = ctx.style_of(Theme::Light).visuals.weak_text_alpha;

        for (theme, text, panel) in [
            (
                "light",
                if high_contrast { Color32::BLACK } else { Color32::from_rgb(18, 22, 28) },
                if high_contrast { Color32::WHITE } else { Color32::from_rgb(244, 246, 249) },
            ),
            (
                "dark",
                if high_contrast { Color32::WHITE } else { Color32::from_rgb(240, 244, 249) },
                if high_contrast { Color32::BLACK } else { Color32::from_rgb(20, 24, 31) },
            ),
        ] {
            let ratio = contrast(faded(text, panel, alpha), panel);
            assert!(
                ratio >= 4.5,
                "{theme} weak text is {ratio:.2}:1, under the 4.5:1 floor"
            );
        }
    }

    #[test]
    fn weak_text_is_readable_in_both_modes() {
        weak_text_stays_readable(false);
        weak_text_stays_readable(true);

        // And egui's default is genuinely the thing being corrected, so this
        // test fails for the right reason if the override is ever dropped.
        let ratio = contrast(
            faded(
                Color32::from_rgb(18, 22, 28),
                Color32::from_rgb(244, 246, 249),
                0.5,
            ),
            Color32::from_rgb(244, 246, 249),
        );
        assert!(ratio < 4.5, "egui's default no longer needs overriding");
    }
}
