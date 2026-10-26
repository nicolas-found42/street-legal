// Streaming harness over the pinned Bochs FPU transcendentals (diagnostic tool, never linked).
// Input records (21 bytes): op, a[10], b[10] (little-endian ext80 images).
// Output records (22 bytes): r0[10], r1[10], sw16 (C0=0x100, C2=0x400, C1=0x200, C3=0x4000).
// op: 0 fsin(a) 1 fcos(a) 2 fpatan(x=a,y=b) 3 f2xm1(a) 4 fyl2x(x=a,y=b) 5 fyl2xp1(x=a,y=b)
//     6 fptan(a) 7 fsincos(a) 8 fscale(ST0=a scaled by ST1=b) 9 frndint(a)
// argv[1] (hex): x87 control word, default 007f. RC applies to ops 8/9; PC applies to ops 8/9 only
// (ops 0..7 always run at precision 80, as Bochs forces for them).
// Always runs at precision 80, round-to-nearest, all exceptions masked: Bochs forces
// FPU_PR_80_BITS for these instructions, as the SDM says PC does not apply to them.
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include "fpu_trans.h"

static floatx80 load(const unsigned char *p) {
    floatx80 r;
    memcpy(&r.signif, p, 8);
    memcpy(&r.signExp, p + 8, 2);
    return r;
}
static void store(unsigned char *p, floatx80 v) {
    memcpy(p, &v.signif, 8);
    memcpy(p + 8, &v.signExp, 2);
}

int main(int argc, char **argv) {
    unsigned cw = argc > 1 ? (unsigned)strtoul(argv[1], 0, 16) : 0x7f;
    unsigned rc = (cw >> 10) & 3, pc = (cw >> 8) & 3;
    unsigned char in[21], out[22];
    while (fread(in, 1, 21, stdin) == 21) {
        memset(out, 0, sizeof out);
        softfloat_status_t st = {};
        st.extF80_roundingPrecision = 80;
        static const uint8_t rcmap[4] = {softfloat_round_near_even, softfloat_round_min, softfloat_round_max, softfloat_round_minMag};
        st.softfloat_roundingMode = in[0] >= 8 ? rcmap[rc] : softfloat_round_near_even;
        if (in[0] >= 8) st.extF80_roundingPrecision = pc == 0 ? 32 : pc == 2 ? 64 : 80;
        st.softfloat_exceptionMasks = 0x3f;
        floatx80 a = load(in + 1), b = load(in + 11), r0 = a, r1 = a;
        unsigned short sw = 0;
        switch (in[0]) {
        case 0: if (fsin(r0, st) == -1) sw |= 0x400; break;
        case 1: if (fcos(r0, st) == -1) sw |= 0x400; break;
        case 2: r0 = fpatan(a, b, st); break;
        case 3: r0 = f2xm1(a, st); break;
        case 4: r0 = fyl2x(a, b, st); break;
        case 5: r0 = fyl2xp1(a, b, st); break;
        case 6: if (fptan(r0, st) == -1) sw |= 0x400; r1.signif = 0x8000000000000000ULL; r1.signExp = 0x3fff; break;
        case 8: r0 = extF80_scale(a, b, &st); break;
        case 9: r0 = extF80_roundToInt(a, &st); break;
        case 7: if (fsincos(a, &r0, &r1, st) == -1) sw |= 0x400; break;
        }
        store(out, r0);
        store(out + 10, r1);
        memcpy(out + 20, &sw, 2);
        fwrite(out, 1, 22, stdout);
    }
    return 0;
}
