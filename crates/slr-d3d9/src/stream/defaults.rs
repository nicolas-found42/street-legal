//! Documented D3D9 default device state (Microsoft D3DRENDERSTATETYPE /
//! D3DTEXTURESTAGESTATETYPE / D3DSAMPLERSTATETYPE reference), identical to the
//! table the headless oracle's `d3d9_device.py` initialises its device from.
//! Floats are bit patterns. `None` means the default depends on hardware or
//! creation parameters and is treated as "unset": any explicit set is visible.

fn f(v: f32) -> u32 {
    v.to_bits()
}

/// Default of render state `state`. `auto_depth` is
/// `D3DPRESENT_PARAMETERS::EnableAutoDepthStencil` (default ZENABLE).
pub fn render_state(state: u32, auto_depth: Option<bool>) -> Option<u32> {
    Some(match state {
        7 => return auto_depth.map(u32::from),
        8 => 3,
        9 => 2,
        14..=16 => u32::from(state != 15),
        19 => 2,
        20 => 1,
        22 => 3,
        23 => 4,
        24..=29 | 34 | 35 => match state {
            25 => 8,
            _ => 0,
        },
        36 => f(0.0),
        37 | 38 => f(1.0),
        48 | 52 => 0,
        53..=55 => 1,
        56 => 8,
        57 => 0,
        58..=60 => 0xFFFF_FFFF,
        128..=135 | 198..=205 => 0,
        136 | 137 => 1,
        139 | 140 => 0,
        141 | 142 => 1,
        143 => 0,
        145 => 1,
        146 => 2,
        147 | 148 | 151 | 152 => 0,
        154 | 155 => f(1.0),
        156 | 157 => 0,
        158 => f(1.0),
        159 | 160 => f(0.0),
        161 => 1,
        162 => 0xFFFF_FFFF,
        163 | 165 => 0,
        166 => return None,
        167 => 0,
        168 => 0xF,
        170 => f(0.0),
        171 => 1,
        172 => 3,
        173 => 1,
        174 => 0,
        175 => f(0.0),
        176 => 0,
        178 | 179 => f(1.0),
        180..=183 => f(0.0),
        184 | 185 => 0,
        186..=188 => 1,
        189 => 8,
        190..=192 => 0xF,
        193 => 0xFFFF_FFFF,
        194 => 0,
        195 => f(0.0),
        206 => 0,
        207 => 2,
        208 | 209 => 1,
        _ => return None,
    })
}

pub fn texture_stage_state(stage: u32, kind: u32) -> Option<u32> {
    Some(match kind {
        1 => {
            if stage == 0 {
                4
            } else {
                1
            }
        }
        2 => 2,
        3 => 1,
        4 => {
            if stage == 0 {
                2
            } else {
                1
            }
        }
        5 => 2,
        6 => 1,
        7..=10 => f(0.0),
        11 => stage,
        22 | 23 => f(0.0),
        24 => 0,
        26..=28 => 1,
        32 => 0,
        _ => return None,
    })
}

pub fn sampler_state(kind: u32) -> Option<u32> {
    Some(match kind {
        1..=3 => 1,
        4 => 0,
        5 | 6 => 1,
        7 => 0,
        8 => f(0.0),
        9 => 0,
        10 => 1,
        11 | 12 => 0,
        13 => 256,
        _ => return None,
    })
}

pub const IDENTITY: [u8; 64] = {
    let one = 1.0f32.to_bits().to_le_bytes();
    let mut m = [0u8; 64];
    let mut i = 0;
    while i < 4 {
        let o = i * 20;
        m[o] = one[0];
        m[o + 1] = one[1];
        m[o + 2] = one[2];
        m[o + 3] = one[3];
        i += 1;
    }
    m
};

/// Name of render state `n` (D3DRS_*), for inventories and divergence text.
pub fn render_state_name(n: u32) -> String {
    let name = match n {
        7 => "ZENABLE",
        8 => "FILLMODE",
        9 => "SHADEMODE",
        14 => "ZWRITEENABLE",
        15 => "ALPHATESTENABLE",
        16 => "LASTPIXEL",
        19 => "SRCBLEND",
        20 => "DESTBLEND",
        22 => "CULLMODE",
        23 => "ZFUNC",
        24 => "ALPHAREF",
        25 => "ALPHAFUNC",
        26 => "DITHERENABLE",
        27 => "ALPHABLENDENABLE",
        28 => "FOGENABLE",
        29 => "SPECULARENABLE",
        34 => "FOGCOLOR",
        35 => "FOGTABLEMODE",
        36 => "FOGSTART",
        37 => "FOGEND",
        38 => "FOGDENSITY",
        48 => "RANGEFOGENABLE",
        52 => "STENCILENABLE",
        53 => "STENCILFAIL",
        54 => "STENCILZFAIL",
        55 => "STENCILPASS",
        56 => "STENCILFUNC",
        57 => "STENCILREF",
        58 => "STENCILMASK",
        59 => "STENCILWRITEMASK",
        60 => "TEXTUREFACTOR",
        128..=135 => "WRAPn",
        136 => "CLIPPING",
        137 => "LIGHTING",
        139 => "AMBIENT",
        140 => "FOGVERTEXMODE",
        141 => "COLORVERTEX",
        142 => "LOCALVIEWER",
        143 => "NORMALIZENORMALS",
        145 => "DIFFUSEMATERIALSOURCE",
        146 => "SPECULARMATERIALSOURCE",
        147 => "AMBIENTMATERIALSOURCE",
        148 => "EMISSIVEMATERIALSOURCE",
        151 => "VERTEXBLEND",
        152 => "CLIPPLANEENABLE",
        154 => "POINTSIZE",
        155 => "POINTSIZE_MIN",
        156 => "POINTSPRITEENABLE",
        157 => "POINTSCALEENABLE",
        158 => "POINTSCALE_A",
        159 => "POINTSCALE_B",
        160 => "POINTSCALE_C",
        161 => "MULTISAMPLEANTIALIAS",
        162 => "MULTISAMPLEMASK",
        163 => "PATCHEDGESTYLE",
        165 => "DEBUGMONITORTOKEN",
        166 => "POINTSIZE_MAX",
        167 => "INDEXEDVERTEXBLENDENABLE",
        168 => "COLORWRITEENABLE",
        170 => "TWEENFACTOR",
        171 => "BLENDOP",
        172 => "POSITIONDEGREE",
        173 => "NORMALDEGREE",
        174 => "SCISSORTESTENABLE",
        175 => "SLOPESCALEDEPTHBIAS",
        176 => "ANTIALIASEDLINEENABLE",
        178 => "MINTESSELLATIONLEVEL",
        179 => "MAXTESSELLATIONLEVEL",
        180 => "ADAPTIVETESS_X",
        181 => "ADAPTIVETESS_Y",
        182 => "ADAPTIVETESS_Z",
        183 => "ADAPTIVETESS_W",
        184 => "ENABLEADAPTIVETESSELLATION",
        185 => "TWOSIDEDSTENCILMODE",
        186..=189 => "CCW_STENCIL",
        190 => "COLORWRITEENABLE1",
        191 => "COLORWRITEENABLE2",
        192 => "COLORWRITEENABLE3",
        193 => "BLENDFACTOR",
        194 => "SRGBWRITEENABLE",
        195 => "DEPTHBIAS",
        198..=205 => "WRAP8+",
        206 => "SEPARATEALPHABLENDENABLE",
        207 => "SRCBLENDALPHA",
        208 => "DESTBLENDALPHA",
        209 => "BLENDOPALPHA",
        _ => return format!("RS_{n}"),
    };
    name.to_owned()
}

pub fn texture_stage_name(n: u32) -> String {
    match n {
        1 => "COLOROP",
        2 => "COLORARG1",
        3 => "COLORARG2",
        4 => "ALPHAOP",
        5 => "ALPHAARG1",
        6 => "ALPHAARG2",
        7 => "BUMPENVMAT00",
        8 => "BUMPENVMAT01",
        9 => "BUMPENVMAT10",
        10 => "BUMPENVMAT11",
        11 => "TEXCOORDINDEX",
        22 => "BUMPENVLSCALE",
        23 => "BUMPENVLOFFSET",
        24 => "TEXTURETRANSFORMFLAGS",
        26 => "COLORARG0",
        27 => "ALPHAARG0",
        28 => "RESULTARG",
        32 => "CONSTANT",
        _ => return format!("TSS_{n}"),
    }
    .to_owned()
}

pub fn sampler_name(n: u32) -> String {
    match n {
        1 => "ADDRESSU",
        2 => "ADDRESSV",
        3 => "ADDRESSW",
        4 => "BORDERCOLOR",
        5 => "MAGFILTER",
        6 => "MINFILTER",
        7 => "MIPFILTER",
        8 => "MIPMAPLODBIAS",
        9 => "MAXMIPLEVEL",
        10 => "MAXANISOTROPY",
        11 => "SRGBTEXTURE",
        12 => "ELEMENTINDEX",
        13 => "DMAPOFFSET",
        _ => return format!("SAMP_{n}"),
    }
    .to_owned()
}

pub fn transform_name(n: u32) -> String {
    match n {
        2 => "VIEW".into(),
        3 => "PROJECTION".into(),
        16..=23 => format!("TEXTURE{}", n - 16),
        256..=511 => format!("WORLD{}", n - 256),
        _ => format!("TRANSFORM_{n}"),
    }
}

pub fn primitive_name(n: i64) -> &'static str {
    match n {
        1 => "POINTLIST",
        2 => "LINELIST",
        3 => "LINESTRIP",
        4 => "TRIANGLELIST",
        5 => "TRIANGLESTRIP",
        6 => "TRIANGLEFAN",
        _ => "UNKNOWN",
    }
}
