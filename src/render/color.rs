//! Theme colors as the operator types them: `#rgb`, `#rrggbb`, `#rrggbbaa` or a basic CSS name.

const WHITE: [u8; 4] = [255, 255, 255, 255];

/// The 16 basic CSS color names.
const NAMES: [(&str, [u8; 3]); 16] = [
    ("black", [0, 0, 0]), ("silver", [192, 192, 192]), ("gray", [128, 128, 128]), ("white", [255, 255, 255]),
    ("maroon", [128, 0, 0]), ("red", [255, 0, 0]), ("purple", [128, 0, 128]), ("fuchsia", [255, 0, 255]),
    ("green", [0, 128, 0]), ("lime", [0, 255, 0]), ("olive", [128, 128, 0]), ("yellow", [255, 255, 0]),
    ("navy", [0, 0, 128]), ("blue", [0, 0, 255]), ("teal", [0, 128, 128]), ("aqua", [0, 255, 255]),
];

/// Parse a color to straight (not premultiplied) RGBA. Anything unreadable is white, so text stays visible.
pub fn parse_color(s: &str) -> [u8; 4] {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let digits: Option<Vec<u8>> = hex.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect();
        return match (digits.as_deref(), hex.len()) {
            (Some(&[r, g, b]), 3) => [r * 17, g * 17, b * 17, 255],
            (Some(d), 6) => [d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5], 255],
            (Some(d), 8) => [d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5], d[6] * 16 + d[7]],
            _ => WHITE,
        };
    }
    NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(s)).map(|(_, [r, g, b])| [*r, *g, *b, 255]).unwrap_or(WHITE)
}
