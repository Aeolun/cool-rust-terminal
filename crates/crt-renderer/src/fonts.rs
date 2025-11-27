// ABOUTME: Embedded font data for bundled fonts.
// ABOUTME: All fonts are compiled into the binary for easy distribution.

use crt_core::{BdfFont, Font};

// Embed all TTF fonts at compile time
static IBM_VGA: &[u8] = include_bytes!("../../../assets/fonts/1985-ibm-pc-vga/PxPlus_IBM_VGA8.ttf");
static IBM_BIOS: &[u8] = include_bytes!("../../../assets/fonts/1981-ibm-pc/PxPlus_IBM_BIOS.ttf");
static IBM_3278: &[u8] = include_bytes!("../../../assets/fonts/1971-ibm-3278/3270-Regular.ttf");
static APPLE2: &[u8] = include_bytes!("../../../assets/fonts/1977-apple2/PrintChar21.ttf");
static COMMODORE_PET: &[u8] = include_bytes!("../../../assets/fonts/1977-commodore-pet/PetMe.ttf");
static COMMODORE_64: &[u8] = include_bytes!("../../../assets/fonts/1982-commodore64/C64_Pro_Mono-STYLE.ttf");
static ATARI: &[u8] = include_bytes!("../../../assets/fonts/1979-atari-400-800/AtariClassic-Regular.ttf");
static TERMINUS: &[u8] = include_bytes!("../../../assets/fonts/modern-terminus/TerminusTTF-4.46.0.ttf");
static FIXEDSYS: &[u8] = include_bytes!("../../../assets/fonts/modern-fixedsys-excelsior/FSEX301-L2.ttf");
static PROGGY_TINY: &[u8] = include_bytes!("../../../assets/fonts/modern-proggy-tiny/ProggyTiny.ttf");
static PRO_FONT: &[u8] = include_bytes!("../../../assets/fonts/modern-pro-font-win-tweaked/ProFontWindows.ttf");
static HERMIT: &[u8] = include_bytes!("../../../assets/fonts/modern-hermit/Hermit-medium.otf");
static INCONSOLATA: &[u8] = include_bytes!("../../../assets/fonts/modern-inconsolata/Inconsolata.otf");

// Fallback fonts with good unicode coverage
static FALLBACK_HACK: &[u8] = include_bytes!("../../../assets/fonts/fallback-hack/Hack-Regular.ttf");
static FALLBACK_SYMBOLS: &[u8] = include_bytes!("../../../assets/fonts/fallback-symbols/NotoSansSymbols2-Regular.ttf");
static FALLBACK_EMOJI: &[u8] = include_bytes!("../../../assets/fonts/fallback-emoji/NotoEmoji-VariableFont_wght.ttf");

// Embed BDF (bitmap) fonts at compile time
static BDF_FIXED_6X13: &[u8] = include_bytes!("../../../assets/bdf_fonts/6x13.bdf");
static BDF_FIXED_7X13: &[u8] = include_bytes!("../../../assets/bdf_fonts/7x13.bdf");
static BDF_FIXED_7X14: &[u8] = include_bytes!("../../../assets/bdf_fonts/7x14.bdf");
static BDF_FIXED_8X13: &[u8] = include_bytes!("../../../assets/bdf_fonts/8x13.bdf");
static BDF_FIXED_9X15: &[u8] = include_bytes!("../../../assets/bdf_fonts/9x15.bdf");
static BDF_FIXED_9X18: &[u8] = include_bytes!("../../../assets/bdf_fonts/9x18.bdf");
static BDF_FIXED_10X20: &[u8] = include_bytes!("../../../assets/bdf_fonts/10x20.bdf");
static BDF_AMSTRAD_CPC: &[u8] = include_bytes!("../../../assets/bdf_fonts/amstrad_cpc_extended.bdf");
static BDF_PROFONT_12: &[u8] = include_bytes!("../../../assets/bdf_fonts/profont12.bdf");
static BDF_PROFONT_17: &[u8] = include_bytes!("../../../assets/bdf_fonts/profont17.bdf");
static BDF_COURIER_12: &[u8] = include_bytes!("../../../assets/bdf_fonts/courR12.bdf");
static BDF_COURIER_BOLD_14: &[u8] = include_bytes!("../../../assets/bdf_fonts/courB14.bdf");

/// Get the embedded font data for a given font
pub fn get_font_data(font: Font) -> &'static [u8] {
    match font {
        Font::IbmVga => IBM_VGA,
        Font::IbmBios => IBM_BIOS,
        Font::Ibm3278 => IBM_3278,
        Font::Apple2 => APPLE2,
        Font::CommodorePet => COMMODORE_PET,
        Font::Commodore64 => COMMODORE_64,
        Font::Atari => ATARI,
        Font::Terminus => TERMINUS,
        Font::Fixedsys => FIXEDSYS,
        Font::ProggyTiny => PROGGY_TINY,
        Font::ProFont => PRO_FONT,
        Font::Hermit => HERMIT,
        Font::Inconsolata => INCONSOLATA,
    }
}

/// Get fallback font data for characters missing from the primary font.
/// Returns the rectangular (tall) fallback font - Hack.
pub fn get_fallback_font_data() -> &'static [u8] {
    FALLBACK_HACK
}

/// Get symbols fallback font data for technical symbols.
/// Returns Noto Sans Symbols.
pub fn get_symbols_fallback_font_data() -> &'static [u8] {
    FALLBACK_SYMBOLS
}

/// Get emoji fallback font data for emoji characters.
/// Returns Noto Emoji (monochrome).
pub fn get_emoji_fallback_font_data() -> &'static [u8] {
    FALLBACK_EMOJI
}

/// Get the embedded BDF font data for a given BDF font
pub fn get_bdf_font_data(font: BdfFont) -> &'static [u8] {
    match font {
        BdfFont::Fixed6x13 => BDF_FIXED_6X13,
        BdfFont::Fixed7x13 => BDF_FIXED_7X13,
        BdfFont::Fixed7x14 => BDF_FIXED_7X14,
        BdfFont::Fixed8x13 => BDF_FIXED_8X13,
        BdfFont::Fixed9x15 => BDF_FIXED_9X15,
        BdfFont::Fixed9x18 => BDF_FIXED_9X18,
        BdfFont::Fixed10x20 => BDF_FIXED_10X20,
        BdfFont::AmstradCpc => BDF_AMSTRAD_CPC,
        BdfFont::ProFont12 => BDF_PROFONT_12,
        BdfFont::ProFont17 => BDF_PROFONT_17,
        BdfFont::Courier12 => BDF_COURIER_12,
        BdfFont::CourierBold14 => BDF_COURIER_BOLD_14,
    }
}
