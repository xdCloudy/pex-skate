use std::slice;
use std::sync::OnceLock;

const FMT_DXT1: u32 = 13;
const FMT_DXT5: u32 = 15;
const FLAG_EIGHTBITALPHA: u32 = 0x2000;
const HEADER: usize = 104;

fn mip_count(w: usize, h: usize) -> usize {
    let mut n = 1;
    let (mut a, mut b) = (w, h);
    while a > 1 || b > 1 {
        a = (a / 2).max(1);
        b = (b / 2).max(1);
        n += 1;
    }
    n
}

fn block_bytes(alpha: bool) -> usize {
    if alpha { 16 } else { 8 }
}

fn level_size(w: usize, h: usize, alpha: bool) -> usize {
    ((w + 3) / 4) * ((h + 3) / 4) * block_bytes(alpha)
}

fn lowres_dims(w: usize, h: usize) -> (usize, usize, usize) {
    let mut k = 0;
    let (mut a, mut b) = (w, h);
    while a.max(b) > 16 {
        a = (a / 2).max(1);
        b = (b / 2).max(1);
        k += 1;
    }
    (a, b, k)
}

fn total_size(w: usize, h: usize, alpha: bool) -> usize {
    let (lw, lh, _) = lowres_dims(w, h);
    let mut size = HEADER + level_size(lw, lh, false);
    let (mut a, mut b) = (w, h);
    for _ in 0..mip_count(w, h) {
        size += level_size(a, b, alpha);
        a = (a / 2).max(1);
        b = (b / 2).max(1);
    }
    size
}

fn downsample(src: &[u8], w: usize, h: usize) -> (Vec<u8>, usize, usize) {
    let nw = (w / 2).max(1);
    let nh = (h / 2).max(1);
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            let xs = [(2 * x).min(w - 1), (2 * x + 1).min(w - 1)];
            let ys = [(2 * y).min(h - 1), (2 * y + 1).min(h - 1)];
            let nx = if w > 1 { 2 } else { 1 };
            let ny = if h > 1 { 2 } else { 1 };
            for c in 0..4 {
                let mut sum = 0u32;
                for yy in 0..ny {
                    for xx in 0..nx {
                        sum += src[(ys[yy] * w + xs[xx]) * 4 + c] as u32;
                    }
                }
                let n = (nx * ny) as u32;
                out[(y * nw + x) * 4 + c] = ((sum + n / 2) / n) as u8;
            }
        }
    }
    (out, nw, nh)
}

fn to565(c: [f32; 3]) -> u16 {
    let r = (c[0].clamp(0.0, 255.0) * 31.0 / 255.0 + 0.5) as u16;
    let g = (c[1].clamp(0.0, 255.0) * 63.0 / 255.0 + 0.5) as u16;
    let b = (c[2].clamp(0.0, 255.0) * 31.0 / 255.0 + 0.5) as u16;
    (r.min(31) << 11) | (g.min(63) << 5) | b.min(31)
}

fn from565(v: u16) -> [f32; 3] {
    let r = ((v >> 11) & 31) as u32;
    let g = ((v >> 5) & 63) as u32;
    let b = (v & 31) as u32;
    [((r << 3) | (r >> 2)) as f32, ((g << 2) | (g >> 4)) as f32, ((b << 3) | (b >> 2)) as f32]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

fn palette(c0: u16, c1: u16) -> [[f32; 3]; 4] {
    let a = from565(c0);
    let b = from565(c1);
    let mut p = [a, b, [0.0; 3], [0.0; 3]];
    for i in 0..3 {
        p[2][i] = ((2.0 * a[i] + b[i]) / 3.0).floor();
        p[3][i] = ((a[i] + 2.0 * b[i]) / 3.0).floor();
    }
    p
}

fn indices_for(px: &[[f32; 3]; 16], c0: u16, c1: u16) -> (u32, f32) {
    let p = palette(c0, c1);
    let mut bits = 0u32;
    let mut err = 0.0;
    for (i, c) in px.iter().enumerate() {
        let mut best = 0;
        let mut bd = f32::MAX;
        for (k, q) in p.iter().enumerate() {
            let d = dist(*c, *q);
            if d < bd {
                bd = d;
                best = k;
            }
        }
        err += bd;
        bits |= (best as u32) << (2 * i);
    }
    (bits, err)
}

fn refine(px: &[[f32; 3]; 16], bits: u32) -> Option<([f32; 3], [f32; 3])> {
    let w1 = [1.0f32, 0.0, 2.0 / 3.0, 1.0 / 3.0];
    let (mut aa, mut bb, mut ab) = (0.0f32, 0.0f32, 0.0f32);
    let mut ax = [0.0f32; 3];
    let mut bx = [0.0f32; 3];
    for (i, c) in px.iter().enumerate() {
        let k = ((bits >> (2 * i)) & 3) as usize;
        let a = w1[k];
        let b = 1.0 - a;
        aa += a * a;
        bb += b * b;
        ab += a * b;
        for j in 0..3 {
            ax[j] += a * c[j];
            bx[j] += b * c[j];
        }
    }
    let det = aa * bb - ab * ab;
    if det.abs() < 1e-6 {
        return None;
    }
    let mut e0 = [0.0f32; 3];
    let mut e1 = [0.0f32; 3];
    for j in 0..3 {
        e0[j] = (ax[j] * bb - bx[j] * ab) / det;
        e1[j] = (bx[j] * aa - ax[j] * ab) / det;
    }
    Some((e0, e1))
}

fn expand(v: u32, bits: u32) -> u32 {
    if bits == 5 { (v << 3) | (v >> 2) } else { (v << 2) | (v >> 4) }
}

fn pair_table(bits: u32) -> [(u8, u8); 256] {
    let top = (1u32 << bits) - 1;
    let mut table = [(0u8, 0u8); 256];
    for value in 0..256u32 {
        let mut bd = u32::MAX;
        for e0 in 0..=top {
            for e1 in 0..=top {
                let mix = (2 * expand(e0, bits) + expand(e1, bits)) / 3;
                let d = (mix as i32 - value as i32).unsigned_abs();
                if d < bd {
                    bd = d;
                    table[value as usize] = (e0 as u8, e1 as u8);
                }
            }
        }
    }
    table
}

fn pair_tables() -> &'static ([(u8, u8); 256], [(u8, u8); 256]) {
    static TABLES: OnceLock<([(u8, u8); 256], [(u8, u8); 256])> = OnceLock::new();
    TABLES.get_or_init(|| (pair_table(5), pair_table(6)))
}

fn solid_block(c: [f32; 3], out: &mut [u8]) {
    let (t5, t6) = pair_tables();
    let (r0, r1) = t5[c[0] as usize];
    let (g0, g1) = t6[c[1] as usize];
    let (b0, b1) = t5[c[2] as usize];
    let mut p0 = ((r0 as u16) << 11) | ((g0 as u16) << 5) | b0 as u16;
    let mut p1 = ((r1 as u16) << 11) | ((g1 as u16) << 5) | b1 as u16;
    if p0 < p1 {
        std::mem::swap(&mut p0, &mut p1);
    }
    let exact = to565(c);
    let block = [c; 16];
    let (_, e_err) = indices_for(&block, exact, exact);
    let (mut c0, mut c1, mut bits) = (exact, exact, 0u32);
    if p0 != p1 {
        let (p_bits, p_err) = indices_for(&block, p0, p1);
        if p_err < e_err {
            c0 = p0;
            c1 = p1;
            bits = p_bits;
        }
    }
    out[0..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    out[4..8].copy_from_slice(&bits.to_le_bytes());
}

fn colour_block(px: &[[f32; 3]; 16], out: &mut [u8]) {
    if px.iter().all(|c| *c == px[0]) {
        solid_block(px[0], out);
        return;
    }
    let mut mean = [0.0f32; 3];
    for c in px {
        for j in 0..3 {
            mean[j] += c[j] / 16.0;
        }
    }
    let mut cov = [0.0f32; 6];
    for c in px {
        let d = [c[0] - mean[0], c[1] - mean[1], c[2] - mean[2]];
        cov[0] += d[0] * d[0];
        cov[1] += d[0] * d[1];
        cov[2] += d[0] * d[2];
        cov[3] += d[1] * d[1];
        cov[4] += d[1] * d[2];
        cov[5] += d[2] * d[2];
    }
    let mut lo = [255.0f32; 3];
    let mut hi = [0.0f32; 3];
    for c in px {
        for j in 0..3 {
            lo[j] = lo[j].min(c[j]);
            hi[j] = hi[j].max(c[j]);
        }
    }
    let mut axis = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    for _ in 0..4 {
        let v = [
            cov[0] * axis[0] + cov[1] * axis[1] + cov[2] * axis[2],
            cov[1] * axis[0] + cov[3] * axis[1] + cov[4] * axis[2],
            cov[2] * axis[0] + cov[4] * axis[1] + cov[5] * axis[2],
        ];
        let m = v[0].abs().max(v[1].abs()).max(v[2].abs());
        if m < 1e-6 {
            break;
        }
        axis = [v[0] / m, v[1] / m, v[2] / m];
    }
    let mut c0;
    let mut c1;
    if axis[0].abs() + axis[1].abs() + axis[2].abs() < 1e-6 {
        c0 = to565(mean);
        c1 = c0;
    } else {
        let (mut mn, mut mx) = (f32::MAX, f32::MIN);
        let (mut pmn, mut pmx) = (px[0], px[0]);
        for c in px {
            let t = c[0] * axis[0] + c[1] * axis[1] + c[2] * axis[2];
            if t < mn {
                mn = t;
                pmn = *c;
            }
            if t > mx {
                mx = t;
                pmx = *c;
            }
        }
        c0 = to565(pmx);
        c1 = to565(pmn);
    }
    let (mut bits, mut err) = indices_for(px, c0, c1);
    for _ in 0..2 {
        if c0 == c1 {
            break;
        }
        if let Some((e0, e1)) = refine(px, bits) {
            let (n0, n1) = (to565(e0), to565(e1));
            if n0 == n1 {
                break;
            }
            let (nb, ne) = indices_for(px, n0, n1);
            if ne < err {
                c0 = n0;
                c1 = n1;
                bits = nb;
                err = ne;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    if c0 < c1 {
        std::mem::swap(&mut c0, &mut c1);
        let mut nb = 0u32;
        for i in 0..16 {
            let k = (bits >> (2 * i)) & 3;
            nb |= [1u32, 0, 3, 2][k as usize] << (2 * i);
        }
        bits = nb;
    }
    if c0 == c1 {
        bits = 0;
    }
    out[0..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    out[4..8].copy_from_slice(&bits.to_le_bytes());
}

fn alpha_block(a: &[u8; 16], out: &mut [u8]) {
    let a0 = *a.iter().max().unwrap();
    let a1 = *a.iter().min().unwrap();
    out[0] = a0;
    out[1] = a1;
    let mut bits = 0u64;
    if a0 != a1 {
        let mut p = [0i32; 8];
        p[0] = a0 as i32;
        p[1] = a1 as i32;
        for k in 1..7 {
            p[k + 1] = ((7 - k as i32) * a0 as i32 + k as i32 * a1 as i32) / 7;
        }
        for (i, v) in a.iter().enumerate() {
            let mut best = 0;
            let mut bd = i32::MAX;
            for (k, q) in p.iter().enumerate() {
                let d = (*v as i32 - q).abs();
                if d < bd {
                    bd = d;
                    best = k;
                }
            }
            bits |= (best as u64) << (3 * i);
        }
    }
    out[2..8].copy_from_slice(&bits.to_le_bytes()[0..6]);
}

fn encode_level(px: &[u8], w: usize, h: usize, alpha: bool, out: &mut Vec<u8>) {
    let bw = (w + 3) / 4;
    let bh = (h + 3) / 4;
    let bs = block_bytes(alpha);
    let mut block = vec![0u8; bs];
    for by in 0..bh {
        for bx in 0..bw {
            let mut rgb = [[0.0f32; 3]; 16];
            let mut al = [0u8; 16];
            for y in 0..4 {
                for x in 0..4 {
                    let sx = (bx * 4 + x).min(w - 1);
                    let sy = (by * 4 + y).min(h - 1);
                    let o = (sy * w + sx) * 4;
                    rgb[y * 4 + x] = [px[o] as f32, px[o + 1] as f32, px[o + 2] as f32];
                    al[y * 4 + x] = px[o + 3];
                }
            }
            if alpha {
                alpha_block(&al, &mut block[0..8]);
                colour_block(&rgb, &mut block[8..16]);
            } else {
                colour_block(&rgb, &mut block[0..8]);
            }
            out.extend_from_slice(&block);
        }
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i as usize] = c;
    }
    let mut crc = 0xFFFFFFFFu32;
    for b in data {
        crc = table[((crc ^ *b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

fn srgb_linear(v: u8) -> f64 {
    (v as f64 / 255.0).powf(2.2)
}

fn encode(rgba: &[u8], w: usize, h: usize, alpha: bool, flags: u32) -> Vec<u8> {
    let mut levels = vec![(rgba.to_vec(), w, h)];
    while levels.last().map(|l| l.1 > 1 || l.2 > 1).unwrap_or(false) {
        let (p, a, b) = levels.last().unwrap();
        let next = downsample(p, *a, *b);
        levels.push(next);
    }
    let mut table = [0f64; 256];
    for i in 0..256 {
        table[i] = srgb_linear(i as u8);
    }
    let mut refl = [0f64; 3];
    for p in rgba.chunks_exact(4) {
        for c in 0..3 {
            refl[c] += table[p[c] as usize];
        }
    }
    let n = (w * h) as f64;
    let (lw, lh, k) = lowres_dims(w, h);
    let mut out = Vec::with_capacity(total_size(w, h, alpha));
    out.extend_from_slice(b"VTF\0");
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&5u32.to_le_bytes());
    out.extend_from_slice(&(HEADER as u32).to_le_bytes());
    out.extend_from_slice(&(w as u16).to_le_bytes());
    out.extend_from_slice(&(h as u16).to_le_bytes());
    let flags = flags | if alpha { FLAG_EIGHTBITALPHA } else { 0 };
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    for c in 0..3 {
        out.extend_from_slice(&((refl[c] / n) as f32).to_le_bytes());
    }
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&1.0f32.to_le_bytes());
    out.extend_from_slice(&(if alpha { FMT_DXT5 } else { FMT_DXT1 }).to_le_bytes());
    out.push(levels.len() as u8);
    out.extend_from_slice(&FMT_DXT1.to_le_bytes());
    out.push(lw as u8);
    out.push(lh as u8);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 3]);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    let low_size = level_size(lw, lh, false);
    out.extend_from_slice(&[1, 0, 0, 0]);
    out.extend_from_slice(&(HEADER as u32).to_le_bytes());
    out.extend_from_slice(&[0x30, 0, 0, 0]);
    out.extend_from_slice(&((HEADER + low_size) as u32).to_le_bytes());
    out.extend_from_slice(b"CRC\x02");
    out.extend_from_slice(&crc32(rgba).to_le_bytes());
    let (lp, _, _) = &levels[k];
    encode_level(lp, lw, lh, false, &mut out);
    for (p, a, b) in levels.iter().rev() {
        encode_level(p, *a, *b, alpha, &mut out);
    }
    out
}

#[unsafe(no_mangle)]
pub extern "C" fn skate_vtf_size(w: u32, h: u32, alpha: u32) -> usize {
    total_size(w as usize, h as usize, alpha != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn skate_vtf(rgba: *const u8, w: u32, h: u32, alpha: u32, flags: u32, out: *mut u8, cap: usize) -> isize {
    if rgba.is_null() || out.is_null() || w == 0 || h == 0 {
        return -1;
    }
    let (w, h) = (w as usize, h as usize);
    let src = unsafe { slice::from_raw_parts(rgba, w * h * 4) };
    let data = encode(src, w, h, alpha != 0, flags);
    if data.len() > cap {
        return -(data.len() as isize);
    }
    let dst = unsafe { slice::from_raw_parts_mut(out, data.len()) };
    dst.copy_from_slice(&data);
    data.len() as isize
}
