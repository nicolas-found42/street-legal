// Independent CPU execution of5516c0's six x87 instructions, restoring the
// complete incoming legacy x87 environment and all eight physical registers.
// Build for x86_64 (Rosetta on Apple Silicon). No game code or capture is run.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static void put16(unsigned char *p, uint16_t v) { memcpy(p, &v, 2); }
static uint16_t get16(const unsigned char *p) { uint16_t v; memcpy(&v, p, 2); return v; }
int main(int argc, char **argv) {
    if (argc != 24) return 2;
    unsigned char incoming[108] = {0};
    _Alignas(16) unsigned char outgoing[512] = {0};
    uint16_t cw = (uint16_t)strtoul(argv[1], NULL, 0);
    uint16_t sw = (uint16_t)strtoul(argv[2], NULL, 0);
    uint16_t tags = (uint16_t)strtoul(argv[3], NULL, 0);
    unsigned top = (sw >> 11) & 7;
    put16(incoming, cw); put16(incoming + 4, sw); put16(incoming + 8, tags);
    for (unsigned i = 0; i < 8; ++i) {
        unsigned physical = (top + i) & 7;
        uint64_t significand = strtoull(argv[4 + physical * 2], NULL, 0);
        uint16_t exponent = (uint16_t)strtoul(argv[5 + physical * 2], NULL, 0);
        memcpy(incoming + 28 + i * 10, &significand, 8);
        put16(incoming + 36 + i * 10, exponent);
    }
    int64_t frequency = strtoll(argv[20], NULL, 0);
    int64_t baseline = strtoll(argv[21], NULL, 0);
    int64_t counter = strtoll(argv[22], NULL, 0);
    uint32_t scale_bits = (uint32_t)strtoul(argv[23], NULL, 0);
    float scale; memcpy(&scale, &scale_bits, 4);
    __asm__ volatile (
        "frstor %[incoming]\n\t"
        "fildll %[counter]\n\t"
        "fildll %[baseline]\n\t"
        "fsubrp %%st, %%st(1)\n\t"
        "fildll %[frequency]\n\t"
        "fdivrp %%st, %%st(1)\n\t"
        "fmuls %[scale]\n\t"
        "fxsave %[outgoing]\n\t"
        "fninit\n\t"
        : [outgoing] "=m" (outgoing)
        : [incoming] "m" (incoming), [counter] "m" (counter),
          [baseline] "m" (baseline), [frequency] "m" (frequency), [scale] "m" (scale)
        : "st", "st(1)", "st(2)", "memory"
    );
    sw = get16(outgoing + 2); top = (sw >> 11) & 7;
    tags=0;
    for(unsigned physical=0;physical<8;++physical){
        unsigned logical=(physical+8-top)&7;
        uint64_t significand; memcpy(&significand,outgoing+32+logical*16,8);
        uint16_t exponent=get16(outgoing+40+logical*16)&0x7fff;
        unsigned tag=!(outgoing[4]&(1u<<physical))?3:exponent==0?(significand==0?1:2):exponent==0x7fff?2:0;
        tags|=tag<<(physical*2);
    }
    printf("{\"control\":%u,\"status\":%u,\"tags\":%u,\"registers\":[", get16(outgoing), sw, tags);
    for (unsigned physical = 0; physical < 8; ++physical) {
        unsigned logical = (physical + 8 - top) & 7;
        uint64_t significand; memcpy(&significand, outgoing + 32 + logical * 16, 8);
        printf("%s[%llu,%u]", physical ? "," : "", (unsigned long long)significand, get16(outgoing + 40 + logical * 16));
    }
    puts("]}");
    return 0;
}
