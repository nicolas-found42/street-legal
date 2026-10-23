/* Streaming FSIN/FCOS value probe, built as x86_64 and run under Rosetta 2.
 * Reports translated x87, not physical Intel execution. Inputs are raw m32
 * words; outputs are complete tbyte register images with C2. No math library.
 */
#include <stdint.h>
#include <stdio.h>

static void emit(const unsigned char *bytes) {
    for (int i = 0; i < 10; ++i) printf("%02x", bytes[i]);
}

int main(void) {
    uint32_t word;
    const unsigned short cw = 0x007f;
    while (scanf("%x", &word) == 1) {
        unsigned char sine[16] = {0}, cosine[16] = {0};
        unsigned short sin_sw, cos_sw;
        __asm__ volatile (
            "fninit\n\tfldcw %3\n\tflds %2\n\tfsin\n\tfnstsw %1\n\tfstpt %0"
            : "=m" (sine), "=m" (sin_sw) : "m" (word), "m" (cw) : "st");
        __asm__ volatile (
            "fninit\n\tfldcw %3\n\tflds %2\n\tfcos\n\tfnstsw %1\n\tfstpt %0"
            : "=m" (cosine), "=m" (cos_sw) : "m" (word), "m" (cw) : "st");
        printf("%08x ", word); emit(sine); printf(" "); emit(cosine);
        printf(" %u\n", ((sin_sw | cos_sw) & 0x0400) != 0);
    }
    return ferror(stdin) || ferror(stdout);
}
