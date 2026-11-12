//! The app logo, an abacus, drawn at any size. Shared by the app (window + sidebar) and build.rs (.exe icon).

/// Straight-alpha RGBA pixels, `size`×`size`. The design lives on a 64-unit grid; each pixel is
/// 4×4 supersampled so small sizes stay smooth.
pub fn abacus(size: u32) -> Vec<u8> {
    const WOOD: [u8; 3] = [139, 90, 43];
    const ROD: [u8; 3] = [190, 190, 190];
    const BEADS: [([u8; 3], [f32; 4]); 3] =
        [([200, 50, 50], [13.5, 23.5, 43.5, 51.5]), ([230, 170, 30], [13.5, 33.5, 41.5, 51.5]), ([30, 140, 140], [13.5, 21.5, 29.5, 51.5])];
    // Topmost shape wins: beads over rods over the frame.
    let color_at = |x: f32, y: f32| -> Option<[u8; 3]> {
        for (row, (c, xs)) in BEADS.iter().enumerate() {
            let cy = 18.5 + 14.0 * row as f32;
            if xs.iter().any(|cx| (x - cx).powi(2) + (y - cy).powi(2) <= 30.0) {
                return Some(*c);
            }
        }
        if (6.0..58.0).contains(&x) && [18.0, 32.0, 46.0].iter().any(|r| (*r..r + 1.0).contains(&y)) {
            return Some(ROD);
        }
        let outer = (2.0..62.0).contains(&x) && (4.0..60.0).contains(&y);
        let inner = (6.0..58.0).contains(&x) && (8.0..56.0).contains(&y);
        (outer && !inner).then_some(WOOD)
    };
    let k = 64.0 / size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (mut sum, mut hits) = ([0u32; 3], 0u32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let (x, y) = ((px as f32 + (sx as f32 + 0.5) / 4.0) * k, (py as f32 + (sy as f32 + 0.5) / 4.0) * k);
                    if let Some(c) = color_at(x, y) {
                        (0..3).for_each(|i| sum[i] += c[i] as u32);
                        hits += 1;
                    }
                }
            }
            let avg = |i: usize| sum[i].checked_div(hits).unwrap_or(0) as u8;
            out.extend_from_slice(&[avg(0), avg(1), avg(2), (hits * 255 / 16) as u8]);
        }
    }
    out
}

/// A Windows .ico holding the logo at each size (32-bit BMP entries).
#[allow(dead_code)] // only build.rs calls this
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&s| {
            let rgba = abacus(s);
            let mask_row = (s.div_ceil(32) * 4) as usize;
            let mut b = Vec::new();
            for v in [40u32, s, s * 2] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.extend_from_slice(&1u16.to_le_bytes());
            b.extend_from_slice(&32u16.to_le_bytes());
            b.extend_from_slice(&[0u8; 24]); // BI_RGB, no palette
            for y in (0..s).rev() {
                for x in 0..s {
                    let i = ((y * s + x) * 4) as usize;
                    b.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
                }
            }
            b.resize(b.len() + mask_row * s as usize, 0); // AND mask unused with 32-bit alpha
            b
        })
        .collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, img) in sizes.iter().zip(&images) {
        let d = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[d, d, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    images.iter().for_each(|i| out.extend_from_slice(i));
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_shapes() {
        let px = super::abacus(64);
        assert_eq!(px.len(), 64 * 64 * 4);
        assert_eq!(px[3], 0, "corner is transparent");
        let ico = super::ico(&[16, 256]);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
        assert_eq!(ico[6 + 16], 0, "256 px is written as 0 in the directory");
    }
}
