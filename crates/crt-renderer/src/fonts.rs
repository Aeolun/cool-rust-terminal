// ABOUTME: Embedded font data for bundled fonts.
// ABOUTME: All fonts are compiled into the binary for easy distribution.

use crt_core::Font;

// Embed all fonts at compile time
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
