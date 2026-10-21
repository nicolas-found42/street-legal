//! Original46e320 base defaults and446180 car defaults before CFG dispatch.
//! Constants below are stores from those functions, independent of captured
//! heap snapshots. Resource references are resolved through the live cfg Host.
use slr_physics::{
    cfg::{self, Arg, ExtCall, Host, This},
    space::Space,
};
fn stores(m: &mut Space, base: u32, words: &[(u32, u32)]) {
    for &(offset, value) in words {
        m.w32(base + offset, value);
    }
}
fn lookup_bind(m: &mut Space, host: &mut dyn Host, slot: u32, name: &str, index: u32, kind: u32) {
    let handle = host.call(
        m,
        &ExtCall {
            f: cfg::EXT_LOOKUP,
            this: This::None,
            a: vec![Arg::S(name.into()), Arg::U(index)],
        },
    );
    host.call(
        m,
        &ExtCall {
            f: cfg::EXT_BIND,
            this: This::Addr(slot),
            a: vec![Arg::U(handle), Arg::U(kind), Arg::U(0)],
        },
    );
}
pub fn base_defaults(m: &mut Space, k: u32, host: &mut dyn Host) {
    stores(
        m,
        k,
        &[
            (0x114c, 0),
            (0x1150, 0),
            (0xfa8, 0),
            (0xff8, 0),
            (0x103c, 0),
            (0xff0, u32::MAX),
            (0x11dc, 0x4b189680),
            (0x11d8, 0x3f800000),
            (0x11d4, 0x3f800000),
            (0x11e0, 0x3d4ccccd),
            (0x11e4, 0x3e800000),
            (0x1210, 0),
            (0x1148, 0x3f800000),
            (0xff4, u32::MAX),
            (0x11b8, 0),
            (0x11bc, 0),
            (0x11c0, 0x3ecccccd),
            (0x11c4, 0),
            (0xffc, u32::MAX),
        ],
    );
    lookup_bind(m, host, k + 0x1138, "decals.rpk", 11, 7);
    let id = m.r32(k + 0xc);
    let name = host.call(
        m,
        &ExtCall {
            f: cfg::EXT_RESNAME,
            this: This::None,
            a: vec![Arg::U(id)],
        },
    );
    m.w32(k + 0x124, name);
}
pub fn car_defaults(m: &mut Space, k: u32, host: &mut dyn Host) -> u32 {
    base_defaults(m, k, host);
    // Literal scalar stores446198..446a1f and447275..447296.
    stores(
        m,
        k,
        &[
            (0x114c, 2),
            (0x11d8, 0x41200000),
            (0x11dc, 0x4cbebc20),
            (0x11e0, 0x3ecccccd),
            (0x11e4, 0x3dcccccd),
            (0x12f8, 0),
            (0xff4, 0xffffffff),
            (0x128c, 0x3e4ccccd),
            (0x1290, 0x41200000),
            (0x1280, 0x41a00000),
            (0x1284, 0x42c80000),
            (0x1288, 0x3f800000),
            (0x1b84, 0x404f5c29),
            (0x1b88, 0xc019999a),
            (0x1b8c, 0xc065c28f),
            (0x1b3c, 0x3f6e147b),
            (0x1b40, 0x3f4ccccd),
            (0x1b44, 0),
            (0x1a7c, 1),
            (0x1a64, 0x3fb70a3d),
            (0x1a68, 0xc08b851f),
            (0x1a6c, 0xc0651eb8),
            (0x1a9c, 0x3ef5c28f),
            (0x1aa0, 0xb8d1b717),
            (0x1aa4, 0xbef5c28f),
            (0x1aa8, 0x3ef5c28f),
            (0x1ac4, 1),
            (0x1aac, 0x3f800000),
            (0x1ab0, 0xbf8ccccd),
            (0x1ab4, 0),
            (0x1ae4, 0x40200000),
            (0x1ae8, 0xbca3d70a),
            (0x1aec, 0xc00ccccd),
            (0x1af0, 0x400ccccd),
            (0x1b0c, 2),
            (0x1af4, 0x3f800000),
            (0x1af8, 0xbf8ccccd),
            (0x1afc, 0),
            (0x1b2c, 0x40200000),
            (0x1b30, 0xbca3d70a),
            (0x1b34, 0xc00ccccd),
            (0x1b38, 0x400ccccd),
            (0x1c74, 2),
            (0x1c88, 0xff4fe015),
            (0x1c5c, 0x3f0ccccd),
            (0x1c60, 0x3f5c28f6),
            (0x1c64, 0),
            (0x1be4, 2),
            (0x1bf8, 0xffd0d0ff),
            (0x1bcc, 0x3f3851ec),
            (0x1bd0, 0x3f4147ae),
            (0x1bd4, 0),
            (0x1c10, 0x47c34f80),
            (0x1c2c, 2),
            (0x1c40, 0xff4fe015),
            (0x1c14, 0x3e99999a),
            (0x1c18, 0x3f5c28f6),
            (0x1c1c, 0),
            (0x1c58, 0x4479c000),
            (0x1cbc, 2),
            (0x1cd0, 0xff000000),
            (0x1ca4, 0xbe2e147b),
            (0x1ca8, 0x3f4ccccd),
            (0x1cac, 0),
            (0x1ce0, 0x41000000),
            (0x1ce8, 0x41000000),
            (0x1d04, 2),
            (0x1d18, 0xff000000),
            (0x1cec, 0xbe2e147b),
            (0x1cf0, 0x3f68f5c3),
            (0x1cf4, 0),
            (0x1d28, 0x41000000),
            (0x1d30, 0x41000000),
            (0x1d4c, 2),
            (0x1d60, 0xff4fe015),
            (0x1d34, 0xbf000000),
            (0x1d38, 0x3f400000),
            (0x1d3c, 0),
            (0x1d78, 0x497423f0),
            (0x1d94, 4),
            (0x1da8, 0xffff0000),
            (0x1d7c, 0x404f5c29),
            (0x1d80, 0xc019999a),
            (0x1d84, 0xc0651eb8),
            (0x1ddc, 4),
            (0x1df0, 0xffff0000),
            (0x1dc4, 0x404f5c29),
            (0x1dc8, 0xc019999a),
            (0x1dcc, 0xc0651eb8),
            (0x1e24, 4),
            (0x1e38, 0xffff0000),
            (0x1e0c, 0x404f5c29),
            (0x1e10, 0xc019999a),
            (0x1e14, 0xc0651eb8),
            (0x1e6c, 4),
            (0x1e80, 0xffff0000),
            (0x1e54, 0x404f5c29),
            (0x1e58, 0xc019999a),
            (0x1e5c, 0xc0651eb8),
            (0x19ec, 1),
            (0x19d4, 0xbecccccd),
            (0x19d8, 0x3dcccccd),
            (0x19dc, 0xbf333333),
            (0x1a0c, 0x400ccccd),
            (0x1a10, 0xba03126f),
            (0x1a14, 0xc00ccccd),
            (0x1a18, 0x400ccccd),
            (0x1a34, 1),
            (0x1a1c, 0xbe99999a),
            (0x1a20, 0x3dcccccd),
            (0x1a24, 0xbf333333),
            (0x1a54, 0x400ccccd),
            (0x1a58, 0xbca3d70a),
            (0x1a5c, 0xc00ccccd),
            (0x1a60, 0x400ccccd),
            (0x177c, 0),
            (0x1780, 0xffffffff),
            (0x1f40, 0),
            (0x2bc4, 0),
            (0x12ec, 0),
            (0x12f0, 0),
            (0x12f4, 0),
            (0x12a4, 0x3f666666),
            (0x12ac, 0x3f666666),
            (0x1298, 0),
            (0x129c, 0),
            (0x12b0, 0),
            (0x12a0, 0x3ecccccd),
            (0x12b4, 0),
            (0x12a8, 0x3e99999a),
            (0x12b8, 0),
            (0x12dc, 0xbf800000),
            (0x12e0, 0xbf800000),
            (0x12e4, 0xbf800000),
            (0x12e8, 0xbf800000),
            (0x12bc, 0),
            (0x12c0, 0),
            (0x12c4, 0x3f800000),
            (0x12c8, 0),
            (0x12cc, 0x4049999a),
            (0x12d0, 0x3c23d70a),
            (0x12d4, 0xbf800000),
            (0x12d8, 0xbf800000),
            (0x2e94, 0),
            (0x1ec4, 0xbf800000),
            (0x2e4c, 0),
            (0x1eac, 0),
            (0x1eb0, 0),
            (0x1eb4, 0),
            (0x1eb8, 0),
            (0x1ebc, 0),
            (0x1ec0, 0),
        ],
    );
    for (offset, text) in [
        (0x19c8, "NabcdefR"),
        (0x1c8c, "%c"),
        (0x1bfc, "%4.0f"),
        (0x1c44, "%03.0f"),
        (0x1cd4, "%01.0f"),
        (0x1d1c, "%01.0f"),
        (0x1d64, "%07.0f"),
    ] {
        for (i, b) in text.bytes().chain([0]).enumerate() {
            m.w8(k + offset + i as u32, b);
        }
    }
    for offset in [
        0x1b90, 0x1a70, 0x1ab8, 0x1b00, 0x1d88, 0x1dd0, 0x1e18, 0x1e60, 0x19e0, 0x1a28,
    ] {
        stores(m, k + offset, &[(0, 0), (4, 0), (8, 0)]);
    }
    // Lookup order follows446229..44693d, including repeated retained references.
    for (slot, name, index, kind) in [
        (0x1e9c, "frontend.rpk", 0x23, 14),
        (0x1ba0, "cars.rpk", 0x55, 14),
        (0x1b58, "cars.rpk", 0x57, 14),
        (0x1a80, "cars.rpk", 0x56, 14),
        (0x1c78, "cars.rpk", 0x58, 14),
        (0x1c30, "cars.rpk", 0x58, 14),
        (0x1cc0, "cars.rpk", 0x5c, 14),
        (0x1d08, "cars.rpk", 0x5c, 14),
        (0x1d50, "cars.rpk", 0x58, 14),
        (0x1d98, "cars.rpk", 0x61, 14),
        (0x1de0, "cars.rpk", 0x62, 14),
        (0x1e28, "cars.rpk", 0x63, 14),
        (0x1e70, "cars.rpk", 0x64, 14),
        (0x1ed0, "sound.rpk", 0xd, 6),
        (0x1ee0, "sound.rpk", 0xd, 6),
        (0x1ef0, "sound.rpk", 0xd, 6),
        (0x1f00, "sound.rpk", 0xd, 6),
        (0x1f10, "sound.rpk", 0x19, 6),
        (0x1f20, "sound.rpk", 0x1a, 6),
    ] {
        lookup_bind(m, host, k + slot, name, index, kind);
    }
    // Parameter block constructors42cec0 zero each embedded Ref;45dc70 is a no-op.
    let p = m.alloc(0xc1c);
    m.w32(k + 0x2e48, p);
    stores(
        m,
        p,
        &[
            (0x338, 0),
            (0x33c, 0x3f800000),
            (0x580, 0),
            (0x584, 0x3f800000),
            (0x7c8, 0),
            (0x7cc, 0x3f800000),
            (0xcc, 0x42480000),
            (0xd0, 0x43480000),
            (0xd4, 0x43960000),
            (0xd8, 0x42c80000),
            (0xdc, 0x43480000),
            (0xe0, 0x41200000),
            (0xe4, 0x447a0000),
            (0xe8, 0x42480000),
            (0xec, 0x42a78d36),
            (0xf0, 0x38d1b717),
            (0xf4, 0x3cf5c28f),
            (0x10, 0x459c4000),
            (0x7d0, 0x3dcccccd),
            (0x804, 0x44af0000),
            (0x808, 0x44af0000),
            (0x80c, 0x43480000),
            (0x810, 0x461c4000),
            (0x878, 0x3f800000),
            (0x8d4, 0x41200000),
            (0x8d8, 0x3f800000),
            (0x8, 5),
            (0x1c, 4),
            (0x20, 0),
            (0x24, 0x40800000),
            (0x28, 0x402ccccd),
            (0x2c, 0x3fc00000),
            (0x30, 0x3f800000),
            (0x3c, 0xc0c00000),
            (0x44, 0),
            (0xc8, 0x43fa0000),
            (0x40, 0x40400000),
            (0x14, 0x45dac000),
            (0x18, 0x461c4000),
            (0x908, 0),
            (0x90c, 0x41200000),
            (0x910, 0x40600000),
            (0x914, 0x40a00000),
            (0x918, 0x3f000000),
            (0xa0c, 0x3f333333),
            (0xa10, 0x402ccccd),
            (0xa14, 0x3e4ccccd),
            (0xa18, 0),
            (0xa1c, 0x40000000),
            (0xa20, 0x3f800000),
            (0xa24, 0x3dcccccd),
            (0xa28, 0),
            (0xa2c, 0),
            (0xa30, 0),
            (0xa34, 0x42480000),
            (0xa38, 0),
            (0xa3c, 0x42300000),
            (0xa40, 0x3f800000),
            (0xa44, 0x3f800000),
            (0xa48, 0x41200000),
            (0xa4c, 0x42480000),
            (0xa50, 0x3c23d70a),
            (0xa54, 0),
            (0xc18, 0),
            (0, 5),
            (0x8fc, 0),
            (0x900, 0),
            (0x904, 0),
            (0x91c, 0xbecccccd),
            (0x920, 0xbe99999a),
            (0x924, 0),
            (0x928, 0),
            (0x92c, 0x3e99999a),
            (0x930, 0),
        ],
    );
    for (slot, name, index, kind) in [
        (0x7d4, "sound.rpk", 7, 6),
        (0x7e4, "sound.rpk", 9, 6),
        (0x7f4, "sound.rpk", 10, 6),
        (0x894, "sound.rpk", 0x17, 6),
        (0x8a4, "sound.rpk", 0x18, 6),
        (0x844, "sound.rpk", 0x15, 6),
        (0x8b4, "sound.rpk", 8, 6),
        (0x8c4, "sound.rpk", 6, 6),
        (0x814, "sound.rpk", 0x25, 6),
        (0x8dc, "particles.rpk", 2, 14),
        (0x8ec, "particles.rpk", 2, 14),
    ] {
        lookup_bind(m, host, p + slot, name, index, kind);
    }
    for slot in [4, 12] {
        let curve = m.alloc(5 * 8);
        m.w32(p + slot, curve);
        for (i, (x, y)) in [
            (0x43480000, 0),
            (0x44bb8000, 0x3f4ccccd),
            (0x457a0000, 0x3f800000),
            (0x459c4000, 0x3f4ccccd),
            (0x45dac000, 0),
        ]
        .into_iter()
        .enumerate()
        {
            m.w32(curve + i as u32 * 8, x);
            m.w32(curve + i as u32 * 8 + 4, y);
        }
    }
    // Original static block60e068, copied by447270 to K+2e50.
    stores(
        m,
        k + 0x2e50,
        &[
            (0, 0x3fb33333),
            (4, 0),
            (8, 0x3fbeb852),
            (12, 0),
            (16, 0x41733333),
            (20, 0),
            (24, 0),
            (28, 0),
            (32, 0xbf800000),
            (36, 0),
            (40, 0),
            (44, 0x45fa0000),
            (48, 0x3f800000),
            (52, 0x3c75c28f),
            (56, 0x3ecccccd),
            (60, 0),
            (64, 0),
        ],
    );
    p
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_preparse_default_image_and_external_calls() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-defaults-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("optional original defaults probe absent");
            return;
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let k = value["k"].as_u64().unwrap() as u32;
        let mut m = Space::new();
        m.set_heap_ptr(value["before"]["heap"].as_u64().unwrap() as u32);
        for row in value["before"]["words"].as_array().unwrap() {
            m.w32(
                row[0].as_u64().unwrap() as u32,
                row[1].as_u64().unwrap() as u32,
            );
        }
        let calls = value["after"]["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(cfg::test_support::parse_call)
            .collect();
        let mut host = cfg::test_support::ReplayHost::new(calls);
        car_defaults(&mut m, k, &mut host);
        assert!(host.exhausted());
        let end = value["after"]["heap"].as_u64().unwrap() as u32;
        assert_eq!(m.heap_ptr(), end);
        let expected = value["after"]["words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r[0].as_u64().unwrap() as u32, r[1].as_u64().unwrap() as u32))
            .collect::<std::collections::HashMap<_, _>>();
        for a in (slr_physics::space::HEAP_BASE..end).step_by(4) {
            assert_eq!(
                m.r32(a),
                expected.get(&a).copied().unwrap_or(0),
                "word at {a:#x} K offset {:#x}",
                a.wrapping_sub(k)
            );
        }
    }
}
