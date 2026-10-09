"""Decode RFG .cpeg_pc/.gpeg_pc texture banks (GEKV v10, DXT1 as observed on Mason) to PNG.

Format (Research/evidence_registry.md E-013): cpeg header 0x18 bytes; 0x30-byte records at 0x18: u32 data offset
into the gpeg, u16 width, u16 height, ..., byte at +0x1b format class (7/8/9 seen = DXT1 256/512/1024),
u32 at +0x1c byte size of the mip chain; names follow the records as NUL-terminated strings.
u16 at +0x08 pixel format: 400 (0x190) = DXT1, 402 (0x192) = DXT5 (block size 16; eyelashes, vfx).
Usage: python Extractors/rfg_peg.py <prefix> <outdir>   (prefix without extension)
Outputs must resolve inside LocalData.
"""
import struct, sys
from pathlib import Path
import numpy as np
from PIL import Image

PROJECT = Path(__file__).resolve().parents[1]


def dxt1(data, w, h):
    bw, bh = (w + 3) // 4, (h + 3) // 4
    blk = np.frombuffer(data[:bw * bh * 8], dtype=np.uint8).reshape(bh, bw, 8)
    c = blk[..., :4].copy().view('<u2').reshape(bh, bw, 2).astype(np.uint32)
    def rgb(v):
        r = (v >> 11) & 31; g = (v >> 5) & 63; b = v & 31
        return np.stack([(r * 255 + 15) // 31, (g * 255 + 31) // 63, (b * 255 + 15) // 31], -1).astype(np.float32)
    c0, c1 = rgb(c[..., 0]), rgb(c[..., 1])
    pal = np.zeros((bh, bw, 4, 4), np.float32)
    pal[..., 0, :3] = c0; pal[..., 1, :3] = c1
    four = (c[..., 0] > c[..., 1])[..., None]
    pal[..., 2, :3] = np.where(four, (2 * c0 + c1) / 3, (c0 + c1) / 2)
    pal[..., 3, :3] = np.where(four, (c0 + 2 * c1) / 3, 0)
    pal[..., :, 3] = 255
    pal[..., 3, 3] = np.where(four[..., 0], 255, 0)
    bits = blk[..., 4:].copy().view('<u4').reshape(bh, bw)
    idx = (bits[..., None] >> (2 * np.arange(16, dtype=np.uint32))) & 3          # (bh,bw,16)
    px = np.take_along_axis(pal, idx[..., None].astype(np.int64), axis=2)         # (bh,bw,16,4)
    px = px.reshape(bh, bw, 4, 4, 4).transpose(0, 2, 1, 3, 4).reshape(bh * 4, bw * 4, 4)
    return px[:h, :w].astype(np.uint8)


def dxt5(data, w, h):
    """DXT5: 8 bytes of interpolated alpha, then a DXT1 colour block (always 4-colour)."""
    bw, bh = (w + 3) // 4, (h + 3) // 4
    blk = np.frombuffer(data[:bw * bh * 16], dtype=np.uint8).reshape(bh, bw, 16)
    colour = blk[..., 8:].reshape(-1).tobytes()
    img = dxt1(colour, bw * 4, bh * 4)
    # DXT1 decoding treats c0 <= c1 as 3-colour + transparent; DXT5 colour blocks are always 4-colour.
    c = blk[..., 8:12].copy().view('<u2').reshape(bh, bw, 2).astype(np.int32)
    three = c[..., 0] <= c[..., 1]
    if three.any():
        fixed = blk[..., 8:].copy()
        swap = fixed[three]
        # Swap endpoints and remap indices (0<->1, 2<->3) to get an equivalent 4-colour block.
        swap[:, [0, 1, 2, 3]] = swap[:, [2, 3, 0, 1]]
        bits = swap[:, 4:].copy().view('<u4')[:, 0]
        swap[:, 4:] = (bits ^ np.uint32(0x55555555)).view(np.uint8).reshape(-1, 4)
        fixed[three] = swap
        img = dxt1(fixed.reshape(-1).tobytes(), bw * 4, bh * 4)
    a0 = blk[..., 0].astype(np.float32); a1 = blk[..., 1].astype(np.float32)
    k = np.arange(8, dtype=np.float32)
    eight = (a0 > a1)[..., None]
    pal = np.where(eight,
                   np.where(k == 0, a0[..., None], np.where(k == 1, a1[..., None], ((8 - k) * a0[..., None] + (k - 1) * a1[..., None]) / 7)),
                   np.where(k == 0, a0[..., None], np.where(k == 1, a1[..., None],
                            np.where(k < 6, ((6 - k) * a0[..., None] + (k - 1) * a1[..., None]) / 5, np.where(k == 6, 0, 255)))))
    raw = blk[..., 2:8].astype(np.uint64)
    bits = sum(raw[..., i] << np.uint64(8 * i) for i in range(6))
    idx = (bits[..., None] >> (np.uint64(3) * np.arange(16, dtype=np.uint64))) & np.uint64(7)
    alpha = np.take_along_axis(pal, idx.astype(np.int64), axis=2)
    alpha = alpha.reshape(bh, bw, 4, 4).transpose(0, 2, 1, 3).reshape(bh * 4, bw * 4)
    img = img.copy()
    img[..., 3] = np.round(alpha).astype(np.uint8)
    return img[:h, :w]


def decode(t):
    """RGBA pixels of a texture record from read_peg."""
    if t['format'] == 402:
        return dxt5(t['data'], t['w'], t['h'])
    if t['format'] == 400:
        return dxt1(t['data'], t['w'], t['h'])
    raise ValueError(f"{t['name']}: unsupported pixel format {t['format']}")


def read_peg(prefix):
    cc = Path(prefix + '.cpeg_pc').read_bytes(); gg = Path(prefix + '.gpeg_pc').read_bytes()
    magic, ver, total, hdr, recsz, count = struct.unpack_from('<4sIIHHI', cc, 0)
    assert magic == b'GEKV' and count and (total == len(cc))
    names = cc[0x18 + 0x30 * count:].split(b'\0')
    out = []
    for i in range(count):
        b = 0x18 + 0x30 * i
        off, w, h, pixel = struct.unpack_from('<IHHH', cc, b)
        size = struct.unpack_from('<I', cc, b + 0x1c)[0]
        out.append(dict(name=names[i].decode(), w=w, h=h, off=off, size=size, fmt=cc[b + 0x1b], format=pixel, data=gg[off:off + size]))
    return out


if __name__ == '__main__':
    out = Path(sys.argv[2]).resolve()
    assert out.is_relative_to(PROJECT / 'LocalData')
    out.mkdir(parents=True, exist_ok=True)
    for t in read_peg(sys.argv[1]):
        img = decode(t)
        name = Path(t['name']).stem + '.png'
        Image.fromarray(img).save(out / name); print(name, t['w'], t['h'], t['fmt'])
