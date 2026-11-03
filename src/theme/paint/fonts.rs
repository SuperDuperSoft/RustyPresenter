//! Extra bundled faces for rich text: Noto Sans Bold and Italic, so bold and italic runs look
//! the same on every computer (Noto Sans Medium/SemiBold/Regular are already in `render`).
use crate::render::Fonts;

/// Noto Sans Bold (700) and Italic (400), the same 2.015 hinted release as the bundled Regular.
pub const RICH_FACES: [&[u8]; 2] = [
    include_bytes!("../../../assets/fonts/NotoSans-Bold.ttf"),
    include_bytes!("../../../assets/fonts/NotoSans-Italic.ttf"),
];

/// Adds the bold and italic faces to a font database (called from `Fonts::new` and
/// `Fonts::bundled_only` in Part B).
pub fn add_rich_faces(fonts: &mut Fonts) {
    for face in RICH_FACES { fonts.sys.db_mut().load_font_data(face.to_vec()); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_text::fontdb;

    #[test]
    fn bold_and_italic_noto_faces_are_available_after_adding() {
        let mut f = Fonts::bundled_only();
        add_rich_faces(&mut f);
        let has = |w: u16, italic: bool| f.sys.db().faces().any(|x| x.families.iter().any(|(n, _)| n == "Noto Sans") && x.weight.0 == w && (x.style == fontdb::Style::Italic) == italic);
        assert!(has(700, false)); assert!(has(400, true)); assert!(has(600, false));
    }
}
