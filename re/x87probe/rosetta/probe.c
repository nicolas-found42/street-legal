/* Rosetta x87 probe runner (win32 console, i686).
 *
 * Executes every row of vectors.inc exactly like run_unicorn.py:
 * fresh FPCW per row from (PC, RC), FLD tbyte operands in, one instruction
 * stub, ST(0)/memory/flags read back exactly. Prints one JSON result per
 * line on stdout (same schema as the Unicorn runner where applicable).
 *
 * Build (mingw-w64): see build.sh. Run under Wine; capture stdout to
 * rosetta_results.jsonl and feed to compare_rosetta.py.
 */
#include <stdio.h>
#include <string.h>

struct vec {
    int id;
    int op; /* 0=fadd 1=fsub 2=fmul 3=fdiv 4=fsqrt 5=fsin 6=fcos 7=f2xm1
               8=fyl2x 9=fpatan 10=fprem 11=fcom 12=fcomp 13=fucom 14=fucomi
               15=fld 16=fst 17=fistp */
    int pc;
    int rc;
    int a_sign, a_kind, a_exp;
    unsigned long long a_sig;
    int b_sign, b_kind, b_exp;
    unsigned long long b_sig;
};

#include "vectors.inc"

/* 16-byte slot: 10-byte tbyte image + 6 pad */
static unsigned char M80[16];
static unsigned char M80B[16];
static unsigned int M32;
static unsigned long long M64;

static void set_cw(unsigned short cw) { __asm__ __volatile__ ("fldcw %0" : : "m" (cw)); }
static unsigned short get_sw(void) { unsigned short sw = 0; __asm__ __volatile__ ("fstsw %0" : "=m" (sw)); return sw; }
static unsigned get_eflags(void) {
    unsigned f = 0;
#if defined(__x86_64__)
    {
        unsigned long long wide = 0;
        __asm__ __volatile__ ("pushfq; popq %0" : "=r" (wide));
        f = (unsigned)wide;
    }
#else
    __asm__ __volatile__ ("pushfl; popl %0" : "=r" (f));
#endif
    return f;
}

/* Build the 10-byte tbyte image for (sign,kind,exp,sig) into M80/M80B. */
static void put80(unsigned char *dst, int sign, int kind, int exp, unsigned long long sig) {
    unsigned short w;
    memset(dst, 0, 16);
    if (kind == 1) { /* signed zero must preserve the corpus operand */
        w = (unsigned short)(sign << 15);
        memcpy(dst + 8, &w, 2);
        return;
    }
    if (kind == 2) { /* inf */
        memcpy(dst, "\0\0\0\0\0\0\0\x80", 8);
        w = (unsigned short)(0x7FFF | (sign << 15));
        memcpy(dst + 8, &w, 2);
        return;
    }
    if (kind == 3) { /* NaN: keep the corpus payload and class bit */
        memcpy(dst, &sig, 8);
        w = (unsigned short)(0x7FFF | (sign << 15));
        memcpy(dst + 8, &w, 2);
        return;
    }
    /* num: biased = exp + 16383; subnormals (biased<=0) pass through raw:
       corpus denormals carry exp already normalized or raw unbiased exp;
       M0-07 gen_vectors normalizes f64-subnormals into ext-normal form,
       and hand-made ext-subnormals use biased field (exp = biased-16383 <= 0
       with integer bit clear). FLD tbyte loads whatever image we give it. */
    {
        int biased = exp + 16383;
        memcpy(dst, &sig, 8);
        w = (unsigned short)((biased & 0x7FFF) | (sign << 15));
        memcpy(dst + 8, &w, 2);
    }
}

static void fld_slot(const unsigned char *slot) {
    __asm__ __volatile__ ("fldt %0" : : "m" (*(const unsigned char (*)[16])slot) : "st(7)");
}

/* result holders */
static unsigned char STB[16];

static void fstp_slot(unsigned char *slot) {
    __asm__ __volatile__ ("fstpt %0" : "=m" (*(unsigned char (*)[16])slot) : : "st(7)");
}

static void run_fld_fst_add(const unsigned char *a, const unsigned char *b, int opbyte1, int opbyte2) {
    /* FLD a, FLD b, then 2-byte op (D8 C1 etc.), then FSTP tbyte STB */
    unsigned char op1 = (unsigned char)opbyte1, op2 = (unsigned char)opbyte2;
    fld_slot(a);
    fld_slot(b);
    __asm__ __volatile__ (".byte 0xD8, 0xC1" : : "m" (op1), "m" (op2) : "st(7)");
    (void)op1; (void)op2;
    fstp_slot(STB);
}

int main(void) {
    int i;
    setvbuf(stdout, NULL, _IOFBF, 65536);
    /* Start from a known-empty register stack (mingw startup may leave junk
       tags; each row pushes 1-2 and pops exactly 1, so one FINIT up front
       keeps the depth constant instead of drifting toward stack fault). */
    __asm__ __volatile__ ("fninit" ::: "st(7)");
    for (i = 0; i < NROWS; i++) {
        const struct vec *v = &ROWS[i];
        unsigned pcbits = (v->pc == 24) ? 0 : (v->pc == 53) ? 2 : 3;
        /* 0x037F has PC=64 baked in; clear PC+RC bits before setting. */
        unsigned short cw = (unsigned short)((0x037F & ~0x0F00) | (pcbits << 8) | ((v->rc & 3) << 10));
        unsigned short sw;
        unsigned long long sig;
        unsigned short expw;
        int sign;
        int biased;
        /* FNINIT per row: binary-op cases push 2 but pop 1, so without a
           reset the stack drifts to overflow (row 7+ faulted: SW IE/SF).
           FNINIT resets CW too, so set_cw must come after. */
        __asm__ __volatile__ ("fninit" ::: "st(7)");
        set_cw(cw);
        put80(M80, v->a_sign, v->a_kind, v->a_exp, v->a_sig);
        put80(M80B, v->b_sign, v->b_kind, v->b_exp, v->b_sig);
        /* dispatch by op; mirrors run_unicorn.py load orders */
        switch (v->op) {
        case 0: /* fadd D8 C1: load a then b => ST0=b,ST1=a; ST0=a+b */
            fld_slot(M80); fld_slot(M80B);
            __asm__ __volatile__ (".byte 0xD8, 0xC1" ::: "st(7)");
            fstp_slot(STB);
            break;
        case 1: /* fsub = FSUBR D8 E9: ST0 = ST1-ST0 = a-b */
            fld_slot(M80); fld_slot(M80B);
            __asm__ __volatile__ (".byte 0xD8, 0xE9" ::: "st(7)");
            fstp_slot(STB);
            break;
        case 2: /* fmul D8 C9 */
            fld_slot(M80); fld_slot(M80B);
            __asm__ __volatile__ (".byte 0xD8, 0xC9" ::: "st(7)");
            fstp_slot(STB);
            break;
        case 3: /* fdiv = FDIVR D8 F9: ST0 = a/b */
            fld_slot(M80); fld_slot(M80B);
            __asm__ __volatile__ (".byte 0xD8, 0xF9" ::: "st(7)");
            fstp_slot(STB);
            break;
        case 4: /* fsqrt D9 FA */
            fld_slot(M80);
            __asm__ __volatile__ (".byte 0xD9, 0xFA" ::: "st(7)");
            fstp_slot(STB);
            break;
        case 5: /* fsin D9 FE */
            fld_slot(M80);
            __asm__ __volatile__ (".byte 0xD9, 0xFE" ::: "st(7)");
            fstp_slot(STB);
            break;
        case 6: /* fcos D9 FF */
            fld_slot(M80);
            __asm__ __volatile__ (".byte 0xD9, 0xFF" ::: "st(7)");
            fstp_slot(STB);
            break;
        case 7: /* f2xm1 D9 F0 */
            fld_slot(M80);
            __asm__ __volatile__ (".byte 0xD9, 0xF0" ::: "st(7)");
            fstp_slot(STB);
            break;
        case 8: /* fyl2x D9 F1: ST1=y=b, ST0=x=a: load b first then a */
            fld_slot(M80B); fld_slot(M80);
            __asm__ __volatile__ (".byte 0xD9, 0xF1" ::: "st(7)");
            fstp_slot(STB);
            break;
        case 9: /* fpatan D9 F3: ST1=y=a, ST0=x=b: load a then b */
            fld_slot(M80); fld_slot(M80B);
            __asm__ __volatile__ (".byte 0xD9, 0xF3" ::: "st(7)");
            fstp_slot(STB);
            break;
        case 10: { /* fprem D9 F8 + FNSTSW AX */
            unsigned short ax = 0;
            fld_slot(M80B); fld_slot(M80);
            __asm__ __volatile__ (".byte 0xD9, 0xF8" ::: "st(7)");
            __asm__ __volatile__ (".byte 0xDF, 0xE0\n\tmov %%ax, %0" : "=r" (ax) : : "eax");
            fstp_slot(STB);
            sw = get_sw();
            memcpy(&sig, STB, 8); memcpy(&expw, STB + 8, 2);
            sign = (expw >> 15) & 1; biased = expw & 0x7FFF;
            printf("{\"id\":%d,\"op\":\"fprem\",\"pc\":%d,\"rc\":%d,"
                   "\"st\":{\"sign\":%d,\"biased\":%d,\"sig\":\"0x%08lx%08lx\"},"
                   "\"fpsw\":\"0x%04x\",\"ax\":\"0x%04x\","
                   "\"C2\":%d,\"C0\":%d,\"C3\":%d,\"C1\":%d}\n",
                   v->id, v->pc, v->rc, sign, biased,
                   (unsigned long)(sig >> 32), (unsigned long)(sig & 0xFFFFFFFF),
                   sw, ax, (sw >> 10) & 1, (sw >> 8) & 1, (sw >> 14) & 1, (sw >> 9) & 1);
            continue;
        }
        case 11: case 12: case 13: { /* fcom D8 D1 / fcomp D8 D9 / fucom DD E1 + FNSTSW AX */
            unsigned short ax = 0;
            fld_slot(M80B); fld_slot(M80);
            if (v->op == 11) __asm__ __volatile__ (".byte 0xD8, 0xD1" ::: "st(7)");
            else if (v->op == 12) __asm__ __volatile__ (".byte 0xD8, 0xD9" ::: "st(7)");
            else __asm__ __volatile__ (".byte 0xDD, 0xE1" ::: "st(7)");
            __asm__ __volatile__ (".byte 0xDF, 0xE0\n\tmov %%ax, %0" : "=r" (ax) : : "eax");
            /* pop both (fcomp popped one; clean the stack for the next row) */
            __asm__ __volatile__ ("ffree %%st(0)\n\tffree %%st(1)" ::: "st(7)");
            sw = get_sw();
            {
                const char *nm = v->op == 11 ? "fcom" : v->op == 12 ? "fcomp" : "fucom";
                printf("{\"id\":%d,\"op\":\"%s\",\"pc\":%d,\"rc\":%d,"
                       "\"ax\":\"0x%04x\",\"cc\":{\"C3\":%d,\"C2\":%d,\"C0\":%d},"
                       "\"fpsw\":\"0x%04x\"}\n",
                       v->id, nm, v->pc, v->rc, ax,
                       (ax >> 14) & 1, (ax >> 10) & 1, (ax >> 8) & 1, sw);
            }
            /* reset stack pointer: reload empty tags via finit-like cleanup */
            __asm__ __volatile__ ("fninit" ::: "st(7)");
            continue;
        }
        case 14: { /* fucomi: DF F1, EFLAGS */
            unsigned ef;
            fld_slot(M80B); fld_slot(M80);
            __asm__ __volatile__ (".byte 0xDF, 0xF1" ::: "cc", "st(7)");
            ef = get_eflags();
            __asm__ __volatile__ ("fninit" ::: "st(7)");
            sw = get_sw();
            printf("{\"id\":%d,\"op\":\"fucomi\",\"pc\":%d,\"rc\":%d,"
                   "\"variant\":\"fucomi\",\"eflags\":\"0x%08lx\","
                   "\"cc\":{\"CF\":%d,\"PF\":%d,\"ZF\":%d},\"fpsw\":\"0x%04x\"}\n",
                   v->id, v->pc, v->rc, (unsigned long)ef,
                   (ef >> 0) & 1, (ef >> 2) & 1, (ef >> 6) & 1, sw);
            continue;
        }
        case 15: /* fld: load only, read back */
            fld_slot(M80);
            fstp_slot(STB);
            break;
        case 16: { /* fst: FST m64 + FSTP m80 */
            unsigned long long m64v = 0;
            fld_slot(M80);
            __asm__ __volatile__ ("fstl %0" : "=m" (m64v) : : "st(7)");
            fstp_slot(STB);
            memcpy(&sig, STB, 8); memcpy(&expw, STB + 8, 2);
            sign = (expw >> 15) & 1; biased = expw & 0x7FFF;
            sw = get_sw();
            printf("{\"id\":%d,\"op\":\"fst\",\"pc\":%d,\"rc\":%d,"
                   "\"m64\":\"0x%08lx%08lx\","
                   "\"st\":{\"sign\":%d,\"biased\":%d,\"sig\":\"0x%08lx%08lx\"},"
                   "\"fpsw\":\"0x%04x\"}\n",
                   v->id, v->pc, v->rc,
                   (unsigned long)(m64v >> 32), (unsigned long)(m64v & 0xFFFFFFFF),
                   sign, biased,
                   (unsigned long)(sig >> 32), (unsigned long)(sig & 0xFFFFFFFF), sw);
            continue;
        }
        case 17: { /* fistp m32 + m64 (two fresh loads) */
            int i32v = 0;
            long long i64v = 0;
            fld_slot(M80);
            __asm__ __volatile__ ("fistpl %0" : "=m" (i32v) : : "st(7)");
            fld_slot(M80);
            __asm__ __volatile__ ("fistpll %0" : "=m" (i64v) : : "st(7)");
            sw = get_sw();
            printf("{\"id\":%d,\"op\":\"fistp\",\"pc\":%d,\"rc\":%d,"
                   "\"i32\":%d,\"i32_hex\":\"0x%08lx\",\"i64\":%lld,\"i64_hex\":\"0x%08lx%08lx\","
                   "\"fpsw\":\"0x%04x\"}\n",
                   v->id, v->pc, v->rc, i32v, (unsigned long)(unsigned)i32v,
                   i64v, (unsigned long)(i64v >> 32), (unsigned long)(i64v & 0xFFFFFFFF), sw);
            continue;
        }
        default:
            printf("{\"id\":%d,\"error\":\"bad op %d\"}\n", v->id, v->op);
            continue;
        }
        /* common ST0 readout for ops 0-9, 15 */
        sw = get_sw();
        memcpy(&sig, STB, 8); memcpy(&expw, STB + 8, 2);
        sign = (expw >> 15) & 1; biased = expw & 0x7FFF;
        {
            const char *nm = "?";
            switch (v->op) {
            case 0: nm = "fadd"; break;
            case 1: nm = "fsub"; break;
            case 2: nm = "fmul"; break;
            case 3: nm = "fdiv"; break;
            case 4: nm = "fsqrt"; break;
            case 5: nm = "fsin"; break;
            case 6: nm = "fcos"; break;
            case 7: nm = "f2xm1"; break;
            case 8: nm = "fyl2x"; break;
            case 9: nm = "fpatan"; break;
            case 15: nm = "fld"; break;
            }
            printf("{\"id\":%d,\"op\":\"%s\",\"pc\":%d,\"rc\":%d,"
                   "\"st\":{\"sign\":%d,\"biased\":%d,\"sig\":\"0x%08lx%08lx\"},"
                   "\"fpsw\":\"0x%04x\"}\n",
                   v->id, nm, v->pc, v->rc, sign, biased,
                   (unsigned long)(sig >> 32), (unsigned long)(sig & 0xFFFFFFFF), sw);
        }
    }
    return 0;
}
