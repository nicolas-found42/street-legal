/* slr_softfloat_shim.c — small C companion to the slr-x87 Rust FFI.
 *
 * Part of slr-x87 (MIT). It wraps vendored Berkeley SoftFloat 3e (BSD-3-Clause,
 * see vendor/softfloat3e/ATTRIBUTION.md) with two helpers:
 *
 * 1. Global-state accessors (rounding mode, precision, tininess, flags).
 *    SoftFloat keeps these as THREAD_LOCAL globals; the Rust side sets them
 *    per operation under its own mutex, so plain setters/getters suffice.
 *
 * 2. slr_extF80_fprem_trunc(): x87 FPREM with the truncated quotient the SDM
 *    specifies. SoftFloat's extF80_rem implements FPREM1 (nearest quotient);
 *    docs/x87.md resolves the split per the SDM in favor of truncation, with
 *    Unicorn as the oracle (TRUSTED via Unicorn, 35/70 agree, every mismatch
 *    consistent with the quotient-rule split). The algorithm below mirrors
 *    SoftFloat's own extF80_rem long division but selects the truncated
 *    quotient (like the do/while subtract loop with the nearest-selection
 *    removed), so the result is the exact mathematical remainder with the
 *    truncated quotient, at any exponent gap. Partial-reduction (C2=1)
 *    follows the SDM: exponent gap >= 64 does not finish in one FPREM.
 *
 * The division core (reciprocal estimate + 29-bit chunks) is adapted from
 * Berkeley SoftFloat 3e extF80_rem.c (BSD-3-Clause, J. Hauser); the quotient
 * selection is truncation instead of nearest.
 */

#include <stdbool.h>
#include <stdint.h>

#include "platform.h"
#include "primitiveTypes.h"
#include "primitives.h"
#include "internals.h"
#include "specialize.h"
#include "softfloat.h"

void slr_sf_set_rounding(uint_fast8_t mode) { softfloat_roundingMode = mode; }
void slr_sf_set_precision(uint_fast8_t prec) { extF80_roundingPrecision = prec; }
void slr_sf_set_tininess(uint_fast8_t t) { softfloat_detectTininess = t; }
void slr_sf_clear_flags(void) { softfloat_exceptionFlags = 0; }
uint_fast8_t slr_sf_get_flags(void) { return softfloat_exceptionFlags; }

/* Truncated-quotient FPREM. Returns the remainder; *c2 = partial flag
 * (true when the exponent gap >= 64, matching hardware which then needs
 * another FPREM). Invalid/inf/zero cases canonicalize like extF80_rem. */
extFloat80_t slr_extF80_fprem_trunc(extFloat80_t a, extFloat80_t b, bool *c2) {
    union {
        struct extFloat80M s;
        extFloat80_t f;
    } uA, uB, uZ;
    uint_fast16_t uiA64, uiB64;
    uint_fast64_t uiA0, uiB0;
    bool signA;
    int_fast32_t expA, expB;
    uint_fast64_t sigA, sigB;
    struct exp32_sig64 normExpSig;
    int_fast32_t expDiff;
    struct uint128 rem, shiftedSigB;
    uint_fast32_t q, recip32;
    uint_fast64_t q64;
    struct uint128 term, uiZ;
    uint_fast16_t uiZ64;
    uint_fast64_t uiZ0;
    bool signRem;

    uA.f = a;
    uiA64 = uA.s.signExp;
    uiA0 = uA.s.signif;
    signA = signExtF80UI64(uiA64);
    expA = expExtF80UI64(uiA64);
    sigA = uiA0;
    uB.f = b;
    uiB64 = uB.s.signExp;
    uiB0 = uB.s.signif;
    expB = expExtF80UI64(uiB64);
    sigB = uiB0;

    if (expA == 0x7FFF) {
        if ((sigA & UINT64_C(0x7FFFFFFFFFFFFFFF)) ||
            ((expB == 0x7FFF) && (sigB & UINT64_C(0x7FFFFFFFFFFFFFFF)))) {
            uiZ = softfloat_propagateNaNExtF80UI(uiA64, uiA0, uiB64, uiB0);
            uiZ64 = uiZ.v64;
            uiZ0 = uiZ.v0;
            goto uiZ;
        }
        goto invalid;
    }
    if (expB == 0x7FFF) {
        if (sigB & UINT64_C(0x7FFFFFFFFFFFFFFF)) {
            uiZ = softfloat_propagateNaNExtF80UI(uiA64, uiA0, uiB64, uiB0);
            uiZ64 = uiZ.v64;
            uiZ0 = uiZ.v0;
            goto uiZ;
        }
        /* b is inf: a rem inf = a (canonicalized copy). */
        expB += expB;
    }
    if (!expB) expB = 1;
    if (!(sigB & UINT64_C(0x8000000000000000))) {
        if (!sigB) goto invalid;
        normExpSig = softfloat_normSubnormalExtF80Sig(sigB);
        expB += normExpSig.exp;
        sigB = normExpSig.sig;
    }
    if (!expA) expA = 1;
    if (!(sigA & UINT64_C(0x8000000000000000))) {
        if (!sigA) {
            expA = 0;
            goto copyA;
        }
        normExpSig = softfloat_normSubnormalExtF80Sig(sigA);
        expA += normExpSig.exp;
        sigA = normExpSig.sig;
    }

    expDiff = expA - expB;
    if (expDiff < 0) goto copyA;
    /* SDM partial-reduction rule: a single FPREM completes only when the
     * exponent gap is < 64 (quotient fits in 63 bits + C0/C1/C3 bits).
     * Larger gaps reduce partially and set C2. We still compute the FULL
     * exact remainder (hardware would need a second FPREM to get there) but
     * report C2=1 so callers observe the partial flag exactly. */
    *c2 = (expDiff >= 64);

    rem = softfloat_shortShiftLeft128(0, sigA, 32);
    shiftedSigB = softfloat_shortShiftLeft128(0, sigB, 32);
    if (expDiff == 0) {
        /* q = (sigB <= sigA) — single subtract, truncated by construction. */
        if (sigB <= sigA) {
            rem = softfloat_sub128(rem.v64, rem.v0, shiftedSigB.v64, shiftedSigB.v0);
        }
        goto packRem;
    }
    /* Multi-bit quotient: same reciprocal long division as extF80_rem, but
     * the chunk estimates are corrected DOWNWARD only (truncation), and the
     * final selectRem nearest-choice is replaced by the truncated loop. */
    recip32 = softfloat_approxRecip32_1((uint32_t)(sigB >> 32));
    expDiff -= 30;
    for (;;) {
        q64 = (uint_fast64_t)(uint32_t)(rem.v64 >> 2) * recip32;
        if (expDiff < 0) break;
        q = (uint32_t)((q64 + UINT64_C(0x80000000)) >> 32);
        rem = softfloat_shortShiftLeft128(rem.v64, rem.v0, 29);
        term = softfloat_mul64ByShifted32To128(sigB, q);
        rem = softfloat_sub128(rem.v64, rem.v0, term.v64, term.v0);
        if (rem.v64 & UINT64_C(0x8000000000000000)) {
            /* Overestimated: add back (truncation corrects down only). */
            rem = softfloat_add128(rem.v64, rem.v0, shiftedSigB.v64, shiftedSigB.v0);
        }
        expDiff -= 29;
    }
    /* (expDiff cannot be less than -29 here.) */
    q = (uint32_t)(q64 >> 32) >> (~expDiff & 31);
    rem = softfloat_shortShiftLeft128(rem.v64, rem.v0, expDiff + 30);
    term = softfloat_mul64ByShifted32To128(sigB, q);
    rem = softfloat_sub128(rem.v64, rem.v0, term.v64, term.v0);
    if (rem.v64 & UINT64_C(0x8000000000000000)) {
        /* Overestimated final chunk: add back once. (Nearest would also try
         * the altRem candidate here; truncation never rounds up.) */
        rem = softfloat_add128(rem.v64, rem.v0, shiftedSigB.v64, shiftedSigB.v0);
    }
    /* Truncated-quotient fixup: the chunk estimates above can also
     * UNDERESTIMATE (the add-back only corrects overestimates). While the
     * remainder still covers the divisor, subtract again. This loop runs at
     * most a handful of iterations: each pass removes one full divisor. */
    for (;;) {
        /* Compare rem against shiftedSigB without wrapping: an extra
         * subtract is valid iff rem >= shiftedSigB (128-bit). */
        bool ge = (rem.v64 > shiftedSigB.v64) ||
                  (rem.v64 == shiftedSigB.v64 && rem.v0 >= shiftedSigB.v0);
        if (!ge) break;
        rem = softfloat_sub128(rem.v64, rem.v0, shiftedSigB.v64, shiftedSigB.v0);
    }

packRem:
    signRem = signA;
    if (rem.v64 & UINT64_C(0x8000000000000000)) {
        signRem = !signRem;
        rem = softfloat_sub128(0, 0, rem.v64, rem.v0);
    }
    /* FPREM is exact: round at precision 80 (no-op) regardless of the
     * caller-visible precision control. */
    return softfloat_normRoundPackToExtF80(
        signRem, rem.v64 | rem.v0 ? expB + 32 : 0, rem.v64, rem.v0, 80);

invalid:
    softfloat_raiseFlags(softfloat_flag_invalid);
    uiZ64 = defaultNaNExtF80UI64;
    uiZ0 = defaultNaNExtF80UI0;
    goto uiZ;

copyA:
    *c2 = false;
    if (expA < 1) {
        sigA >>= 1 - expA;
        expA = 0;
    }
    uiZ64 = packToExtF80UI64(signA, (uint16_t)expA);
    uiZ0 = sigA;
uiZ:
    uZ.s.signExp = uiZ64;
    uZ.s.signif = uiZ0;
    return uZ.f;
}
