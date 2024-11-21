use num_traits::float::FloatCore;

const GAMMA8: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 5, 5, 5,
    5, 6, 6, 6, 6, 7, 7, 7, 7, 8, 8, 8, 9, 9, 9, 10, 10, 10, 11, 11, 11, 12, 12, 13, 13, 13, 14,
    14, 15, 15, 16, 16, 17, 17, 18, 18, 19, 19, 20, 20, 21, 21, 22, 22, 23, 24, 24, 25, 25, 26, 27,
    27, 28, 29, 29, 30, 31, 32, 32, 33, 34, 35, 35, 36, 37, 38, 39, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 50, 51, 52, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 66, 67, 68, 69, 70, 72,
    73, 74, 75, 77, 78, 79, 81, 82, 83, 85, 86, 87, 89, 90, 92, 93, 95, 96, 98, 99, 101, 102, 104,
    105, 107, 109, 110, 112, 114, 115, 117, 119, 120, 122, 124, 126, 127, 129, 131, 133, 135, 137,
    138, 140, 142, 144, 146, 148, 150, 152, 154, 156, 158, 160, 162, 164, 167, 169, 171, 173, 175,
    177, 180, 182, 184, 186, 189, 191, 193, 196, 198, 200, 203, 205, 208, 210, 213, 215, 218, 220,
    223, 225, 228, 231, 233, 236, 239, 241, 244, 247, 249, 252, 255,
];

pub trait Order {
    fn ordered<P: Pixel>(pixel: &P) -> (u8, u8, u8);
}

pub trait Pixel: Sized {
    fn to_rgb(&self) -> (u8, u8, u8);
    fn from_rgb(rgb: (u8, u8, u8)) -> Self;
    fn to_word<O: Order>(&self) -> u32 {
        let (a, b, c) = O::ordered(self);
        (u32::from(GAMMA8[a as usize]) << 24)
            | (u32::from(GAMMA8[b as usize]) << 16)
            | (u32::from(GAMMA8[c as usize]) << 8)
    }
}

#[allow(clippy::upper_case_acronyms)]
pub struct OrderGRB;
impl Order for OrderGRB {
    fn ordered<P: Pixel>(pixel: &P) -> (u8, u8, u8) {
        let (r, g, b) = pixel.to_rgb();
        (g, r, b)
    }
}

#[allow(clippy::upper_case_acronyms)]
pub struct OrderRGB;
impl Order for OrderRGB {
    fn ordered<P: Pixel>(pixel: &P) -> (u8, u8, u8) {
        let (r, g, b) = pixel.to_rgb();
        (r, g, b)
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Default)]
// All components range 0..=255
pub struct RGB {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Pixel for RGB {
    fn from_rgb((r, g, b): (u8, u8, u8)) -> Self {
        Self { r, g, b }
    }

    fn to_rgb(&self) -> (u8, u8, u8) {
        (self.r, self.g, self.b)
    }
}

pub type Float = f32;
const ONE_THIRD: Float = 1.0 / 3.0;
const TWO_THIRD: Float = 2.0 * ONE_THIRD;
const ONE_SIXTH: Float = 1.0 / 6.0;

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Default)]
// All components range 0..=1.0
pub struct HSL {
    pub h: Float,
    pub s: Float,
    pub l: Float,
}

impl Pixel for HSL {
    fn from_rgb((r, g, b): (u8, u8, u8)) -> Self {
        let r = Float::from(r) / 255.0;
        let g = Float::from(g) / 255.0;
        let b = Float::from(b) / 255.0;

        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        let l = (mx + mn) / 2.0;

        let mut h: Float;
        let s: Float;

        if mx == mn {
            h = 0.0;
            s = 0.0;
        } else {
            let d = mx - mn;
            if l > 0.5 {
                s = d / (2.0 - mx - mn);
            } else {
                s = d / (mx + mn);
            }

            if mx == r {
                h = (g - b) / d + (if g < b { 6.0 } else { 0.0 })
            } else if mx == g {
                h = (b - r) / d + 2.0;
            } else {
                h = (r - g) / d + 4.0;
            }

            h *= ONE_SIXTH;
        }

        Self { h, s, l }
    }

    fn to_rgb(&self) -> (u8, u8, u8) {
        if self.s == 0.0 {
            return (0, 0, 0);
        }

        fn hue2rgb(p: Float, q: Float, mut t: Float) -> Float {
            if t < 0.0 {
                t += 1.0
            }
            if t > 1.0 {
                t -= 1.0
            }

            if t < ONE_SIXTH {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < TWO_THIRD {
                p + (q - p) * (TWO_THIRD - t) * 6.0
            } else {
                p
            }
        }

        fn px(v: Float) -> u8 {
            (v * 255.0).round() as u8
        }

        let q = if self.l < 0.5 {
            self.l * (1.0 + self.s)
        } else {
            self.l + self.s - self.l * self.s
        };
        let p = 2.0 * self.l - q;

        let r = hue2rgb(p, q, self.h + ONE_THIRD);
        let g = hue2rgb(p, q, self.h);
        let b = hue2rgb(p, q, self.h - ONE_THIRD);

        (px(r), px(g), px(b))
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Default)]
// All components range 0..=1.0
pub struct HSV {
    pub h: Float,
    pub s: Float,
    pub v: Float,
}

impl Pixel for HSV {
    fn from_rgb(rgb: (u8, u8, u8)) -> Self {
        let hsl = HSL::from_rgb(rgb);

        let v = hsl.l + hsl.s * hsl.l.min(1.0 - hsl.l);

        let s = if v == 0.0 {
            0.0
        } else {
            2.0 * (1.0 - hsl.l / v)
        };

        Self { h: hsl.h, s, v }
    }

    fn to_rgb(&self) -> (u8, u8, u8) {
        let l = self.v * (1.0 - self.s / 2.0);

        let s = if l == 0.0 || l == 1.0 {
            0.0
        } else {
            (self.v - l) / l.min(1.0 - l)
        };

        HSL { h: self.h, s, l }.to_rgb()
    }
}
