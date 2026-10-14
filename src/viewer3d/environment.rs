use super::{V3, add, scale};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug)]
pub struct Environment {
    id: u64,
    width: usize,
    height: usize,
    rgb: Vec<V3>,
    up: V3,
    down: V3,
}

#[derive(Clone, Debug)]
pub struct EnvironmentLight {
    pub map: Arc<Environment>,
    pub turn: f32,
    pub strength: f32,
    pub backdrop: bool,
}

impl PartialEq for EnvironmentLight {
    fn eq(&self, other: &Self) -> bool {
        self.map.id == other.map.id
            && self.turn == other.turn
            && self.strength == other.strength
            && self.backdrop == other.backdrop
    }
}

impl EnvironmentLight {
    pub fn new(map: Arc<Environment>) -> EnvironmentLight {
        EnvironmentLight { map, turn: 0.0, strength: 1.0, backdrop: true }
    }

    pub(super) fn up(&self) -> V3 {
        scale(self.map.up, self.strength)
    }

    pub(super) fn down(&self) -> V3 {
        scale(self.map.down, self.strength)
    }

    pub(super) fn radiance(&self, d: V3) -> V3 {
        let phi = (d[1].atan2(d[0]) - self.turn).rem_euclid(TAU);
        let theta = d[2].clamp(-1.0, 1.0).acos();
        let u = phi / TAU * self.map.width as f32 - 0.5;
        let v = theta / PI * self.map.height as f32 - 0.5;
        scale(self.map.bilinear(u, v), self.strength)
    }
}

fn texel_direction(u: f32, v: f32) -> V3 {
    let (phi, theta) = (u * TAU, v * PI);
    [theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos()]
}

impl Environment {
    pub fn new(width: usize, height: usize, rgb: Vec<V3>) -> Environment {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let (width, height) = (width.max(1), height.max(1));
        let mut rgb = rgb;
        rgb.resize(width * height, [0.0; 3]);
        let (mut up, mut down) = ([0.0; 3], [0.0; 3]);
        let (mut up_w, mut down_w) = (0.0f32, 0.0f32);
        for y in 0..height {
            let theta = (y as f32 + 0.5) / height as f32 * PI;
            let (st, ct) = theta.sin_cos();
            for x in 0..width {
                let c = rgb[y * width + x].map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
                rgb[y * width + x] = c;
                let w = st * ct.abs();
                if ct >= 0.0 {
                    up = add(up, scale(c, w));
                    up_w += w;
                } else {
                    down = add(down, scale(c, w));
                    down_w += w;
                }
            }
        }
        Environment {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            rgb,
            up: scale(up, 1.0 / up_w.max(1.0e-12)),
            down: scale(down, 1.0 / down_w.max(1.0e-12)),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn rgb(&self) -> &[V3] {
        &self.rgb
    }

    fn bilinear(&self, u: f32, v: f32) -> V3 {
        let (w, h) = (self.width, self.height);
        let v = v.clamp(0.0, h as f32 - 1.0);
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let wrap = |x: f32| (x as i64).rem_euclid(w as i64) as usize;
        let (xa, xb) = (wrap(x0), wrap(x0 + 1.0));
        let (ya, yb) = (y0 as usize, (y0 as usize + 1).min(h - 1));
        let at = |x: usize, y: usize| self.rgb[y * w + x];
        let mix = |a: V3, b: V3, t: f32| [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t);
        mix(mix(at(xa, ya), at(xb, ya), fx), mix(at(xa, yb), at(xb, yb), fx), fy)
    }

    pub fn studio() -> Environment {
        let (w, h) = (512, 256);
        let boxes: [(f32, f32, f32, f32, f32); 3] = [
            (0.9, 0.22, 0.32, 0.14, 9.0),
            (-1.9, 0.38, 0.22, 0.2, 4.0),
            (2.6, 0.3, 0.12, 0.3, 2.5),
        ];
        let rgb = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let d = texel_direction((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                let base =
                    if d[2] >= 0.0 { 0.10 + 0.06 * d[2] } else { 0.05 + 0.03 * (-d[2]).sqrt() };
                let mut c = [base; 3];
                let phi = d[1].atan2(d[0]);
                let elevation = d[2].asin();
                for (az, el, half_w, half_h, power) in boxes {
                    let da = (phi - az + PI).rem_euclid(TAU) - PI;
                    let (ex, ey) = (da.abs() / half_w, (elevation - el).abs() / half_h);
                    let edge = ex.max(ey);
                    if edge < 1.0 {
                        let soft = (1.0 - edge).min(0.15) / 0.15;
                        c = add(c, [power * soft; 3]);
                    }
                }
                c
            })
            .collect();
        Environment::new(w, h, rgb)
    }

    pub fn from_hdr(bytes: &[u8]) -> Result<Environment, String> {
        let mut at = 0;
        let mut line = || -> Result<String, String> {
            let start = at;
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            if at >= bytes.len() {
                return Err("truncated header".into());
            }
            at += 1;
            Ok(String::from_utf8_lossy(&bytes[start..at - 1]).trim().to_string())
        };
        let magic = line()?;
        if !magic.starts_with("#?") {
            return Err("not a Radiance HDR file".into());
        }
        loop {
            let l = line()?;
            if l.is_empty() {
                break;
            }
            if let Some(format) = l.strip_prefix("FORMAT=")
                && format != "32-bit_rle_rgbe"
            {
                return Err(format!("unsupported format {format}"));
            }
        }
        let size = line()?;
        let parts: Vec<&str> = size.split_whitespace().collect();
        let (height, width) = match parts.as_slice() {
            ["-Y", h, "+X", w] => (
                h.parse::<usize>().map_err(|e| e.to_string())?,
                w.parse::<usize>().map_err(|e| e.to_string())?,
            ),
            _ => return Err(format!("unsupported orientation {size}")),
        };
        let mut data = &bytes[at..];
        let mut rgb = Vec::with_capacity(width * height);
        let mut scan = vec![[0u8; 4]; width];
        for _ in 0..height {
            data = read_scanline(data, &mut scan)?;
            rgb.extend(scan.iter().map(|p| {
                if p[3] == 0 {
                    [0.0; 3]
                } else {
                    let f = (p[3] as f32 - 136.0).exp2();
                    [p[0] as f32 * f, p[1] as f32 * f, p[2] as f32 * f]
                }
            }));
        }
        Ok(Environment::new(width, height, rgb))
    }
}

fn read_scanline<'a>(data: &'a [u8], scan: &mut [[u8; 4]]) -> Result<&'a [u8], String> {
    let width = scan.len();
    let short = || "truncated pixels".to_string();
    if (8..0x8000).contains(&width)
        && data.len() >= 4
        && data[0] == 2
        && data[1] == 2
        && data[2] & 0x80 == 0
    {
        if ((data[2] as usize) << 8 | data[3] as usize) != width {
            return Err("scanline width mismatch".into());
        }
        let mut data = &data[4..];
        for channel in 0..4 {
            let mut x = 0;
            while x < width {
                let (&count, rest) = data.split_first().ok_or_else(short)?;
                data = rest;
                if count > 128 {
                    let run = (count - 128) as usize;
                    let (&value, rest) = data.split_first().ok_or_else(short)?;
                    data = rest;
                    if x + run > width {
                        return Err("run overflows scanline".into());
                    }
                    for p in &mut scan[x..x + run] {
                        p[channel] = value;
                    }
                    x += run;
                } else {
                    let run = count as usize;
                    if run == 0 || x + run > width || data.len() < run {
                        return Err("bad literal run".into());
                    }
                    for (p, v) in scan[x..x + run].iter_mut().zip(&data[..run]) {
                        p[channel] = *v;
                    }
                    data = &data[run..];
                    x += run;
                }
            }
        }
        return Ok(data);
    }
    if data.len() < width * 4 {
        return Err(short());
    }
    for (p, chunk) in scan.iter_mut().zip(data.as_chunks::<4>().0) {
        *p = *chunk;
    }
    Ok(&data[width * 4..])
}

pub(super) struct Sampling {
    pub marginal: Vec<f32>,
    pub conditional: Vec<f32>,
}

pub(super) fn sampling(env: &Environment) -> Sampling {
    let (w, h) = (env.width, env.height);
    let mut conditional = vec![0.0f32; w * h];
    let mut rows = vec![0.0f32; h];
    for y in 0..h {
        let st = ((y as f32 + 0.5) / h as f32 * PI).sin();
        let mut sum = 0.0;
        for x in 0..w {
            let c = env.rgb[y * w + x];
            sum += (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) * st + 1.0e-6;
            conditional[y * w + x] = sum;
        }
        rows[y] = sum;
        for x in 0..w {
            conditional[y * w + x] /= sum;
        }
    }
    let mut marginal = vec![0.0f32; h];
    let mut sum = 0.0;
    for y in 0..h {
        sum += rows[y];
        marginal[y] = sum;
    }
    for m in &mut marginal {
        *m /= sum;
    }
    Sampling { marginal, conditional }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uniform_environment_is_the_same_up_and_down_and_everywhere() {
        let env = Environment::new(16, 8, vec![[0.5, 0.25, 1.0]; 128]);
        for k in 0..3 {
            assert!((env.up[k] - env.down[k]).abs() < 1.0e-5);
        }
        let light = EnvironmentLight { strength: 2.0, ..EnvironmentLight::new(Arc::new(env)) };
        let c = light.radiance([0.3, -0.5, 0.8]);
        assert!((c[0] - 1.0).abs() < 1.0e-5 && (c[2] - 2.0).abs() < 1.0e-5, "{c:?}");
    }

    #[test]
    fn the_studio_is_brighter_where_its_softboxes_hang() {
        let light = EnvironmentLight::new(Arc::new(Environment::studio()));
        let (el, az) = (0.22f32, 0.9f32);
        let toward = [el.cos() * az.cos(), el.cos() * az.sin(), el.sin()];
        assert!(light.radiance(toward)[0] > 5.0);
        assert!(light.radiance([0.0, 0.0, -1.0])[0] < 0.2);
        let turned = EnvironmentLight { turn: 1.0, ..light.clone() };
        let az = az + 1.0;
        assert!(turned.radiance([el.cos() * az.cos(), el.cos() * az.sin(), el.sin()])[0] > 5.0);
    }

    #[test]
    fn an_hdr_file_reads_back_flat_and_run_length_encoded() {
        let header = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 2 +X 8\n".to_vec();
        let mut flat = header.clone();
        for i in 0..16u8 {
            flat.extend([128, 64, i * 8, 129]);
        }
        let env = Environment::from_hdr(&flat).unwrap();
        assert_eq!((env.width(), env.height()), (8, 2));
        assert!((env.rgb()[0][0] - 1.0).abs() < 1.0e-6 && (env.rgb()[0][1] - 0.5).abs() < 1.0e-6);
        let mut rle = header;
        for _ in 0..2 {
            rle.extend([2, 2, 0, 8]);
            rle.extend([128 + 8, 128]);
            rle.extend([128 + 8, 64]);
            rle.extend([8, 0, 8, 16, 24, 32, 40, 48, 56]);
            rle.extend([128 + 8, 129]);
        }
        let packed = Environment::from_hdr(&rle).unwrap();
        assert_eq!(packed.rgb()[..8], env.rgb()[..8]);
    }

    #[test]
    fn sampling_tables_climb_to_one() {
        let env = Environment::studio();
        let s = sampling(&env);
        assert!((s.marginal.last().unwrap() - 1.0).abs() < 1.0e-4);
        assert!(s.marginal.windows(2).all(|w| w[0] <= w[1]));
        let row = &s.conditional[..env.width()];
        assert!((row.last().unwrap() - 1.0).abs() < 1.0e-4);
    }
}
