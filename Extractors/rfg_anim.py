"""Decoder for RFG `.anim_pc` clips (ANIM v10), ported from the game's own code (E-017).

Source functions in RFG.exe 1.0.2.1 (addresses; see ReverseEngineering/):
  FUN_00a1e190  stream start: optional track->bone map, optional seek table (flag 0x40)
  FUN_00a1dfe0  per-track layout: rotation blocks/deltas/times, position blocks/deltas/times, extra
  FUN_00a1ac30  rotation sampler (block base + scale * cumulative delta, /16383, slerp)
  FUN_00a1b460  position sampler (i16 base / 64 + i8 delta / 4000, lerp)
  FUN_00a1b9f0  builds the 256-entry per-component scale table at 0x00d9eb50
  FUN_00a1ab40/ab90  key search; key frame numbers are compared against time * 0.0125 (1/80)

Header (little endian):
  0x00 'ANIM', 0x04 u8 version 10, 0x05 u8 flags, 0x06 u16 last frame, 0x0A u8 track count,
  0x0B u8 bone count of the source rig, 0x24 f32 (root travel per loop?), 0x28 u32 offset of the
  track->bone map (0 = identity), 0x2C u32 end of the track data.
Flags: 0x80 u16 counts, 0x40 seek table before tracks, 0x20 extra float3 channel,
  0x10 facial (not handled), 0x02 u16 key times.
Rotations are local quaternions stored inverted; `parse` returns them conjugated as (x, y, z, w).
Positions are offsets added to the bind translation.
Verified on all clips: 2560/2582 parse exactly to the end offset (the rest are facial clips and
two outliers); decoded quaternions of the 1040 player body clips have mean length 1.00005.
"""
import struct

FRAME_RATE = 30.0  # key times are frames; 30 fps per E-007 (1/80 time unit, 2400 ticks/s assumed)
SCALE = {(0, 0): 1, (0, 1): 2, (1, 0): 4, (1, 1): 32}  # (top bit of base byte, flag bit) -> scale


class AnimError(ValueError):
    pass


def _align(p, n):
    return (p + n - 1) & ~(n - 1)


def _s7(b):
    v = b & 0x7F
    return v - 0x80 if v & 0x40 else v


def _s8(b):
    return b - 256 if b >= 128 else b


def _i16(v):
    v &= 0xFFFF
    return v - 0x10000 if v & 0x8000 else v


def _times(d, p, n, wide):
    if wide:
        p = _align(p, 2)
        return list(struct.unpack_from(f'<{n}H', d, p)), p + 2 * n
    return list(d[p:p + n]), p + n


def _track(d, p, flags):
    if flags & 0x80:
        p = _align(p, 2)
        n_rot, n_pos = struct.unpack_from('<HH', d, p)
        p += 4
        n_ext = 0
        if flags & 0x20:
            n_ext = struct.unpack_from('<H', d, p)[0]
            p += 2
    else:
        n_rot, n_pos = d[p], d[p + 1]
        p += 2
        n_ext = 0
        if flags & 0x20:
            n_ext = d[p]
            p += 1
    wide = bool(flags & 0x02)

    rot_blocks, total = [], 0
    while total < n_rot:
        rot_blocks.append(d[p:p + 5])
        total += 1 + (d[p + 4] & 0xF)
        p += 5
    rot_deltas = d[p:p + 4 * n_rot]
    p += 4 * n_rot
    rot_times, p = _times(d, p, n_rot, wide)

    p = _align(p, 2)
    pos_blocks, total = [], 0
    while total < n_pos:
        block = struct.unpack_from('<H3h', d, p)
        pos_blocks.append(block)
        total += block[0]
        p += 8
    pos_deltas = d[p:p + 3 * n_pos]
    p += 3 * n_pos
    pos_times, p = _times(d, p, n_pos, wide)

    if n_ext:
        p = _align(p, 4) + 12 * n_ext
        _, p = _times(d, p, n_ext, wide)

    rotations, k = [], 0
    for h in rot_blocks:
        cum = [0, 0, 0, 0]
        for _ in range((h[4] & 0xF) + 1):
            if k >= n_rot:
                break
            q = []
            for c in range(4):
                cum[c] += _s8(rot_deltas[4 * k + c])
                scale = SCALE[(h[c] >> 7, (h[4] >> (7 - c)) & 1)]
                q.append(_i16(_s7(h[c]) * 256 + scale * cum[c]) / 16383.0)
            # Stored as the inverse (row-vector convention); conjugate to the usual local
            # rotation. Checked with forward kinematics: knees bend forward, hands in front.
            rotations.append((-q[0], -q[1], -q[2], q[3]))
            k += 1

    positions, k = [], 0
    for count, x, y, z in pos_blocks:
        for _ in range(count):
            if k >= n_pos:
                break
            dx, dy, dz = (_s8(v) for v in pos_deltas[3 * k:3 * k + 3])
            positions.append((x / 64 + dx / 4000, y / 64 + dy / 4000, z / 64 + dz / 4000))
            k += 1

    return p, dict(rot_times=rot_times, rotations=rotations, pos_times=pos_times, positions=positions)


def parse(d):
    """Returns dict(frames, tracks=[dict(bone, rot_times, rotations, pos_times, positions)])."""
    if d[:4] != b'ANIM' or d[4] != 10:
        raise AnimError('not ANIM v10')
    flags = d[5]
    if flags & 0x10:
        raise AnimError('facial clip (flag 0x10) not supported')
    count = d[0xA]
    map_offset, end = struct.unpack_from('<II', d, 0x28)
    if map_offset:
        bone_map = list(d[map_offset:map_offset + count])
        p = map_offset + count
    else:
        bone_map = list(range(count))
        p = 0x30
    if flags & 0x40:  # seek table: per-track counts, then N u32 + N u8/u16
        if flags & 0x80:
            p = _align(p, 2)
            counts = struct.unpack_from(f'<{count}H', d, p)
            p += 2 * count
        else:
            counts = d[p:p + count]
            p += count
        n = sum(counts)
        p = _align(p, 4) + 4 * n
        p = _align(p, 2) + 2 * n if flags & 0x02 else p + n
    tracks = []
    for bone in bone_map:
        try:
            p, track = _track(d, p, flags)
        except (struct.error, ValueError, IndexError) as error:
            raise AnimError(f'track {len(tracks)} truncated at {p:#x}') from error
        track['bone'] = bone
        tracks.append(track)
    if p != end:
        raise AnimError(f'track data ends at {p:#x}, header says {end:#x}')
    return dict(frames=struct.unpack_from('<H', d, 6)[0] + 1, tracks=tracks)


def gltf_channels(clip, bones):
    """Per bone: (times, rotations) and (times, translations) in game space.
    Tracks without keys hold the bind pose (identity rotation, bind translation)."""
    out = []
    for t in clip['tracks']:
        b = t['bone']
        if not 0 <= b < len(bones):
            continue  # 0xFF: track not bound to a bone of this rig
        bind = bones[b]['local_t'] if bones[b]['parent'] >= 0 else bones[b]['bind_pos']
        if t['rotations']:
            rot = ([f / FRAME_RATE for f in t['rot_times']], t['rotations'])
        else:
            rot = ([0.0], [(0.0, 0.0, 0.0, 1.0)])
        if t['positions']:
            pos = ([f / FRAME_RATE for f in t['pos_times']],
                   [tuple(a + o for a, o in zip(bind, v)) for v in t['positions']])
        else:
            pos = ([0.0], [tuple(bind)])
        out.append((b, rot, pos))
    return out
