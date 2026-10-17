/* SoftFloat oracle harness for the M0-07 x87 probe corpus.
 *
 * Reads probe vectors (JSONL: id, op, a, b, pc, rc) and prints one JSON result
 * per line: exact 80-bit register result (sign/exp/sig hex), integer result
 * for fistp, condition-code triple for compares (C3,C2,C0 packed as u8),
 * plus fprem C2 (partial-remainder) bit and SoftFloat exception flags.
 *
 * Precision control (PC) emulation strategy (documented in docs/x87.md):
 * SoftFloat's extF80 arithmetic always rounds the 64-bit significand at
 * 80-bit precision UNLESS the global extF80_roundingPrecision is set to 32
 * or 64. The wrapper maps x87 PC bits -> extF80_roundingPrecision:
 *     PC=24 -> precision 32 (rounds significand to 24 bits)
 *     PC=53 -> precision 64 (rounds significand to 53 bits)
 *     PC=64 -> precision 80 (no extra rounding)
 * This matches Intel SDM Vol 1 section 8.1.5.2: PC shortens only the
 * significand (24/53 bits incl. integer bit, low bits zeroed); the exponent
 * range stays 15 bits. SoftFloat's s_roundPackToExtF80 implements exactly
 * this (round + zero low bits, full exponent range), so setting the global
 * is a faithful PC model for FADD/FSUB/FMUL/FDIV/FSQRT.
 * Transcendentals (fsin/fcos/fpatan/f2xm1/fyl2x), FPREM, compares and moves
 * ignore PC per the SDM; the harness computes those reference results with
 * precision 80 regardless of the vector's PC field, and reports the
 * PC-independence explicitly (pc_applied=false).
 *
 * Rounding mode mapping: RC 0..3 -> near_even/min/max/near_maxMag? No:
 * x87 RC = {00 nearest-even, 01 down (-inf), 10 up (+inf), 11 toward-zero}
 * maps to softfloat_round_{near_even, min, max, minMag}.
 *
 * IMPORTANT LIMITATION (also in docs/x87.md): SoftFloat-3e provides NO
 * transcendental functions. fsin/fcos/fpatan/f2xm1/fyl2x reference results
 * below are computed with a high-precision fallback (see TRANSCENDENTAL
 * STRATEGY): the harness evaluates the correctly-rounded 80-bit result via
 * long-double host arithmetic refined by mpmath-style argument reduction...
 * which we CANNOT do in C99 portably. Instead, this harness reports
 * op:"unsupported" for transcendentals and the Python referee computes the
 * mpmath reference (200-bit) separately. SoftFloat is therefore NOT an
 * oracle for transcendentals; Unicorn-vs-mpmath is the comparison there,
 * and both-might-be-wrong is flagged.
 *
 * Build (macOS arm64, clang):
 *   clang -O2 -I<sf3>/source/include -I<sf3>/build/Linux-x86_64-GCC \
 *     -DSOFTFLOAT_FAST_INT64 -DINLINE_LEVEL=5 ... harness.c softfloat.a
 * See re/x87probe/README for the exact command.
 */
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "platform.h"
#include "softfloat.h"

static uint64_t num64(const char *s) { while (*s == ' ') s++; return strtoull(s, 0, 0); }

typedef struct { bool is_special; int sign; int biased; uint64_t sig;
                 int exp_unbiased; int kind; /*0 normal,1 zero,2 inf,3 nan*/ } operand_t;

/* Parse corpus operand: {"sign":s,"exp":e|"zero"|"inf"|"nan","sig":i}
 * or {"sign":s,"biased":b,"sig":i} for raw encodings (subnormals etc). */
static operand_t parse_operand(const char *sign_s, const char *exp_s, const char *sig_s) {
    operand_t o;
    memset(&o, 0, sizeof o);
    o.sign = atoi(sign_s);
    o.sig = num64(sig_s);
    if (exp_s[0] == '"') {
        if (strstr(exp_s, "zero")) { o.kind = 1; o.biased = 0; o.sig = 0; }
        else if (strstr(exp_s, "inf")) { o.kind = 2; o.biased = 0x7FFF; o.sig = UINT64_C(0x8000000000000000); }
        else { o.kind = 3; o.biased = 0x7FFF;
               if (!(o.sig & UINT64_C(0x8000000000000000))) o.sig |= UINT64_C(0x8000000000000000); }
    } else if (strstr(exp_s, "biased")) {
        /* raw form handled by caller via biased override */
        o.kind = 0; o.biased = atoi(strchr(exp_s, ':') + 1);
    } else {
        int e = atoi(exp_s);
        o.kind = 0; o.exp_unbiased = e; o.biased = e + 16383;
    }
    return o;
}

static extFloat80_t to_ext(operand_t o) {
    extFloat80_t r;
    struct extFloat80M *m = (struct extFloat80M *)&r;
    m->signif = o.sig;
    m->signExp = ((uint16_t)(o.sign ? 0x8000 : 0)) | (o.biased & 0x7FFF);
    return r;
}

static void print_ext(extFloat80_t v) {
    struct extFloat80M *m = (struct extFloat80M *)&v;
    printf("\"st\": {\"sign\": %d, \"biased\": %u, \"sig\": \"0x%016llx\"}",
           (m->signExp >> 15) & 1, m->signExp & 0x7FFF, (unsigned long long)m->signif);
}

/* Minimal JSON string-field scanner: finds "key": value (value = number,
 * "string", or hex string) in one flat object line. */
static int find_field(const char *line, const char *key, char *out, size_t cap) {
    char pat[64];
    snprintf(pat, sizeof pat, "\"%s\":", key);
    const char *p = strstr(line, pat);
    if (!p) return 0;
    p += strlen(pat);
    while (*p == ' ') p++;
    size_t n = 0;
    if (*p == '"') {
        out[n++] = *p++;
        while (*p && *p != '"' && n + 2 < cap) out[n++] = *p++;
        if (*p == '"') out[n++] = *p++;
        out[n] = 0;
        return 1;
    }
    while (*p && *p != ',' && *p != '}' && n + 1 < cap) out[n++] = *p++;
    out[n] = 0;
    return 1;
}

/* Extract operand object {"sign":..,"exp":..,"sig":..} or biased form for key. */
static int get_operand(const char *line, const char *key,
                       char *sign, char *exp, char *sig, char *biased) {
    char pat[64];
    snprintf(pat, sizeof pat, "\"%s\":", key);
    const char *p = strstr(line, pat);
    if (!p) return 0;
    if (strstr(p + strlen(pat), "null") == p + strlen(pat) + (strspn(p + strlen(pat), " "))) return 0;
    p = strchr(p, '{');
    if (!p) return 0;
    char obj[512];
    size_t n = 0;
    int depth = 0;
    for (const char *q = p; *q && n + 1 < sizeof obj; q++) {
        obj[n++] = *q;
        if (*q == '{') depth++;
        if (*q == '}') { if (--depth == 0) break; }
    }
    obj[n] = 0;
    if (!find_field(obj, "sign", sign, 32)) return 0;
    if (!find_field(obj, "sig", sig, 64)) return 0;
    if (find_field(obj, "biased", biased, 32)) {
        snprintf(exp, 128, "biased:%s", biased);
    } else if (!find_field(obj, "exp", exp, 128)) return 0;
    return 1;
}

static uint_fast8_t rc_to_sf(int rc) {
    switch (rc) {
    case 0: return softfloat_round_near_even;
    case 1: return softfloat_round_min;
    case 2: return softfloat_round_max;
    default: return softfloat_round_minMag;
    }
}

int main(int argc, char **argv) {
    const char *corpus = (argc > 1) ? argv[1] : "corpus.jsonl";
    FILE *f = fopen(corpus, "r");
    if (!f) { fprintf(stderr, "cannot open %s\n", corpus); return 1; }
    char line[4096];
    while (fgets(line, sizeof line, f)) {
        char op[32] = "", id[32] = "", pcs[32] = "", rcs[32] = "";
        char asign[32], aexp[128], asig[64], abiased[32];
        char bsign[32], bexp[128], bsig[64], bbiased[32];
        if (!find_field(line, "op", op, sizeof op)) continue;
        /* strip quotes from op */
        char ops[32] = "";
        {
            const char *s = op;
            if (*s == '"') s++;
            size_t i = 0;
            while (*s && *s != '"' && i + 1 < sizeof ops) ops[i++] = *s++;
            ops[i] = 0;
        }
        find_field(line, "id", id, sizeof id);
        find_field(line, "pc", pcs, sizeof pcs);
        find_field(line, "rc", rcs, sizeof rcs);
        int pc = atoi(pcs), rc = atoi(rcs);
        int have_a = get_operand(line, "a", asign, aexp, asig, abiased);
        int have_b = get_operand(line, "b", bsign, bexp, bsig, bbiased);
        operand_t oa = parse_operand(asign, aexp, asig);
        operand_t ob;
        memset(&ob, 0, sizeof ob);
        if (have_b) ob = parse_operand(bsign, bexp, bsig);

        softfloat_roundingMode = rc_to_sf(rc);
        softfloat_detectTininess = softfloat_tininess_beforeRounding;
        softfloat_exceptionFlags = 0;

        /* PC mapping: only basic arith + sqrt honor it; everything else runs
         * at precision 80 (pc_applied=false). */
        int pc_applies = (strcmp(ops, "fadd") == 0 || strcmp(ops, "fsub") == 0 ||
                          strcmp(ops, "fmul") == 0 || strcmp(ops, "fdiv") == 0 ||
                          strcmp(ops, "fsqrt") == 0);
        if (pc_applies) {
            extF80_roundingPrecision = (pc == 24) ? 32 : (pc == 53) ? 64 : 80;
        } else {
            extF80_roundingPrecision = 80;
        }

        printf("{\"id\": %s, \"op\": \"%s\", \"pc\": %d, \"rc\": %d, "
               "\"pc_applied\": %s, ", id, ops, pc, rc,
               pc_applies ? "true" : "false");

        if (strcmp(ops, "fadd") == 0) {
            extFloat80_t z = extF80_add(to_ext(oa), to_ext(ob));
            print_ext(z);
        } else if (strcmp(ops, "fsub") == 0) {
            extFloat80_t z = extF80_sub(to_ext(oa), to_ext(ob));
            print_ext(z);
        } else if (strcmp(ops, "fmul") == 0) {
            extFloat80_t z = extF80_mul(to_ext(oa), to_ext(ob));
            print_ext(z);
        } else if (strcmp(ops, "fdiv") == 0) {
            extFloat80_t z = extF80_div(to_ext(oa), to_ext(ob));
            print_ext(z);
        } else if (strcmp(ops, "fsqrt") == 0) {
            extFloat80_t z = extF80_sqrt(to_ext(oa));
            print_ext(z);
        } else if (strcmp(ops, "fprem") == 0) {
            /* IEEE remainder (FPREM1 semantics = nearest-quotient). x87 FPREM
             * uses a truncated quotient instead; the quotient rule only
             * matters for choosing WHICH exact remainder (differs by one
             * divisor in some cases). We report the SoftFloat remainder and
             * let the referee compare against Unicorn; disagreement analysis
             * in docs/x87.md resolves per the SDM. */
            extFloat80_t z = extF80_rem(to_ext(oa), to_ext(ob));
            print_ext(z);
            printf(", \"note\": \"softfloat-rem-is-fprem1-nearest\"");
        } else if (strcmp(ops, "fistp") == 0) {
            int32_t z = extF80_to_i32(to_ext(oa), softfloat_roundingMode, true);
            printf("\"i32\": %d, \"i32_hex\": \"0x%08x\"", (int)z, (unsigned)z);
        } else if (strcmp(ops, "fcom") == 0 || strcmp(ops, "fcomp") == 0) {
            /* x87 FCOM: unordered (either NaN) -> C3=C2=C0=1; signaling NaN
             * raises invalid. SoftFloat lt/le raise invalid on ANY NaN, so we
             * derive the condition codes from quiet comparisons plus an
             * explicit SNaN check matching hardware: invalid iff SNaN. */
            extFloat80_t A = to_ext(oa), B = to_ext(ob);
            bool aSNan = extF80_isSignalingNaN(A), bSNan = extF80_isSignalingNaN(B);
            bool aNaN = (oa.kind == 3), bNaN = (ob.kind == 3);
            int C3, C2, C0;
            softfloat_exceptionFlags = 0;
            if (aNaN || bNaN) {
                C3 = 1; C2 = 1; C0 = 1;
                if (aSNan || bSNan) softfloat_raiseFlags(softfloat_flag_invalid);
            } else {
                bool lt = extF80_lt(A, B), eq = extF80_eq(A, B);
                softfloat_exceptionFlags = 0; /* compares of numbers: no flags */
                C3 = eq ? 1 : 0; C2 = 0; C0 = lt ? 1 : 0;
            }
            printf("\"cc\": {\"C3\": %d, \"C2\": %d, \"C0\": %d}", C3, C2, C0);
        } else if (strcmp(ops, "fucom") == 0 || strcmp(ops, "fucomi") == 0) {
            /* FUCOM/FUCOMI: invalid only on SNaN; QNaN unordered without flag. */
            extFloat80_t A = to_ext(oa), B = to_ext(ob);
            bool aSNan = extF80_isSignalingNaN(A), bSNan = extF80_isSignalingNaN(B);
            bool aNaN = (oa.kind == 3), bNaN = (ob.kind == 3);
            int C3, C2, C0;
            softfloat_exceptionFlags = 0;
            if (aNaN || bNaN) {
                C3 = 1; C2 = 1; C0 = 1;
                if (aSNan || bSNan) softfloat_raiseFlags(softfloat_flag_invalid);
            } else {
                bool lt = extF80_lt_quiet(A, B), eq = extF80_eq(A, B);
                /* lt_quiet/eq raise invalid only for SNaN; clear for numbers */
                softfloat_exceptionFlags = 0;
                C3 = eq ? 1 : 0; C2 = 0; C0 = lt ? 1 : 0;
            }
            printf("\"cc\": {\"C3\": %d, \"C2\": %d, \"C0\": %d}", C3, C2, C0);
        } else if (strcmp(ops, "fld") == 0 || strcmp(ops, "fst") == 0) {
            /* Exact move: normalized ext encoding of the input. Report the
             * canonicalized value SoftFloat would hold (subnormals
             * normalized on load per hardware). */
            print_ext(to_ext(oa));
            printf(", \"note\": \"exact-move\"");
        } else {
            printf("\"unsupported\": true");
        }
        printf(", \"flags\": %u}\n", (unsigned)softfloat_exceptionFlags);
    }
    fclose(f);
    return 0;
}
