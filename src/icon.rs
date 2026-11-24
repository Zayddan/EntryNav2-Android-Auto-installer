//! The app icon, drawn in code. `write_icon_assets` writes it to assets/.

/// Signed distance from `p` to a rounded rectangle centred at (0.5, 0.5).
fn rounded_rect(px: f32, py: f32, half: f32, radius: f32) -> f32 {
    let qx = (px - 0.5).abs() - (half - radius);
    let qy = (py - 0.5).abs() - (half - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

/// Distance from `p` to the segment a–b.
fn segment(px: f32, py: f32, (ax, ay): (f32, f32), (bx, by): (f32, f32)) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
}

/// Distance to the strokes of an "A" centred on `cx`.
fn letter_a(px: f32, py: f32, cx: f32) -> f32 {
    let apex = (cx, 0.28);
    let (left, right) = ((cx - 0.115, 0.72), (cx + 0.115, 0.72));
    let bar_y = 0.57;
    let at = |foot: (f32, f32)| {
        let t = (bar_y - apex.1) / (foot.1 - apex.1);
        (apex.0 + t * (foot.0 - apex.0), bar_y)
    };
    segment(px, py, apex, left)
        .min(segment(px, py, apex, right))
        .min(segment(px, py, at(left), at(right)))
}

/// The icon as RGBA pixels, `size` × `size`, anti-aliased (4×4 samples).
pub fn rgba(size: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let n = 4;
    for y in 0..size {
        for x in 0..size {
            let (mut bg, mut fg) = (0.0f32, 0.0f32);
            for sy in 0..n {
                for sx in 0..n {
                    let px = (x as f32 + (sx as f32 + 0.5) / n as f32) / size as f32;
                    let py = (y as f32 + (sy as f32 + 0.5) / n as f32) / size as f32;
                    if rounded_rect(px, py, 0.46, 0.2) <= 0.0 {
                        bg += 1.0;
                        let d = letter_a(px, py, 0.33).min(letter_a(px, py, 0.67));
                        if d <= 0.038 {
                            fg += 1.0;
                        }
                    }
                }
            }
            let samples = (n * n) as f32;
            let (bg, fg) = (bg / samples, fg / samples);
            let t = y as f32 / size as f32;
            let blue = [79.0 - 24.0 * t, 140.0 - 35.0 * t, 255.0 - 25.0 * t];
            let mix = |c: f32| c * (1.0 - fg / bg.max(1e-6)) + 255.0 * (fg / bg.max(1e-6));
            out.extend_from_slice(&[
                mix(blue[0]) as u8,
                mix(blue[1]) as u8,
                mix(blue[2]) as u8,
                (bg * 255.0) as u8,
            ]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_the_expected_shape() {
        let px = rgba(64);
        assert_eq!(px.len(), 64 * 64 * 4);
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(at(0, 0)[3], 0, "transparent corner");
        assert_eq!(at(32, 4)[3], 255, "opaque background");
        assert!(at(21, 18)[0] > 200, "white letter stroke at the first apex");
        assert!(at(32, 45)[0] < 120, "gap between the two letters");
    }

    fn png(size: u32) -> Vec<u8> {
        let mut data = Vec::new();
        {
            let mut enc = ::png::Encoder::new(&mut data, size, size);
            enc.set_color(::png::ColorType::Rgba);
            enc.set_depth(::png::BitDepth::Eight);
            enc.write_header()
                .unwrap()
                .write_image_data(&rgba(size))
                .unwrap();
        }
        data
    }

    /// Writes assets/icon.ico and assets/icon-<size>.png:
    /// `cargo test write_icon_assets -- --ignored`
    #[test]
    #[ignore]
    fn write_icon_assets() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        std::fs::create_dir_all(&dir).unwrap();
        for size in [16, 32, 64, 128, 256, 512, 1024] {
            std::fs::write(dir.join(format!("icon-{size}.png")), png(size)).unwrap();
        }
        let sizes = [16u32, 24, 32, 48, 64, 256];
        let images: Vec<Vec<u8>> = sizes.iter().map(|s| png(*s)).collect();
        let mut ico = Vec::new();
        ico.extend_from_slice(&[0, 0, 1, 0]);
        ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
        let mut offset = 6 + 16 * sizes.len() as u32;
        for (size, img) in sizes.iter().zip(&images) {
            let dim = if *size >= 256 { 0 } else { *size as u8 };
            ico.extend_from_slice(&[dim, dim, 0, 0]);
            ico.extend_from_slice(&1u16.to_le_bytes());
            ico.extend_from_slice(&32u16.to_le_bytes());
            ico.extend_from_slice(&(img.len() as u32).to_le_bytes());
            ico.extend_from_slice(&offset.to_le_bytes());
            offset += img.len() as u32;
        }
        for img in &images {
            ico.extend_from_slice(img);
        }
        std::fs::write(dir.join("icon.ico"), ico).unwrap();
    }
}
