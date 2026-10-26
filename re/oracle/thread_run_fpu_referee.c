// Independent x87 execution of the Thread.run budget/clock instruction chain.
// Build with clang -arch x86_64 and run through Rosetta on Apple Silicon.
// Inputs are authored numeric platform samples, not executable/snapshot bytes.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
int main(int argc, char **argv) {
    if (argc != 6 && argc != 7) return 2;
    int depth = argc == 7 ? atoi(argv[6]) : 0;
    float retained1 = 1.25f, retained2 = -3.5f;
    uint16_t cw = (uint16_t)strtoul(argv[1], NULL, 0);
    int64_t frequency = strtoll(argv[2], NULL, 0);
    int64_t baseline = strtoll(argv[3], NULL, 0);
    int64_t counter = strtoll(argv[4], NULL, 0);
    float budget = strtof(argv[5], NULL), zero = 0, scale = 1000, deadline;
    uint16_t before, clock, after;
    __asm__ volatile (
        "fninit\n\t"
        "fldcw %[cw]\n\t"
        "testl %[depth], %[depth]\n\t"
        "jz 1f\n\t"
        "flds %[retained1]\n\t"
        "flds %[retained2]\n\t"
        "1:\n\t"
        "flds %[budget]\n\t"
        "fcomps %[zero]\n\t"
        "fnstsw %[before]\n\t"
        "fildll %[counter]\n\t"
        "fildll %[baseline]\n\t"
        "fsubrp %%st, %%st(1)\n\t"
        "fildll %[frequency]\n\t"
        "fdivrp %%st, %%st(1)\n\t"
        "fmuls %[scale]\n\t"
        "fnstsw %[clock]\n\t"
        "fadds %[budget]\n\t"
        "fstps %[deadline]\n\t"
        "fnstsw %[after]\n\t"
        "fninit"
        : [before] "=m" (before), [clock] "=m" (clock), [after] "=m" (after), [deadline] "=m" (deadline)
        : [cw] "m" (cw), [counter] "m" (counter), [baseline] "m" (baseline), [frequency] "m" (frequency),
          [depth] "r" (depth), [retained1] "m" (retained1), [retained2] "m" (retained2), [budget] "m" (budget), [zero] "m" (zero), [scale] "m" (scale)
        : "st", "st(1)", "memory", "cc"
    );
    union { float value; uint32_t bits; } result = { .value = deadline };
    printf("{\"before\":%u,\"clock\":%u,\"after\":%u,\"deadline\":%u}\n", before, clock, after, result.bits);
    return 0;
}
