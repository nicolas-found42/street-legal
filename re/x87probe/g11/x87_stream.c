/* Streaming x87 transcendental probe, built as x86_64. Under Rosetta 2 it measures translated
 * x87; on an Intel Mac (or any x86_64 host) it measures that CPU's own x87 microcode. stderr
 * records which one ran (sysctl.proc_translated, CPU brand). Record formats as bochs_stream.cc.
 * Control word 0x007F by default (PC24, RC nearest, all masked); argv[1] hex overrides.
 * For fptan the second result is the pushed 1.0 (ST0), the first is the tangent (ST1).
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/sysctl.h>

int main(int argc, char **argv) {
    int translated = 0;
    size_t length = sizeof translated;
    int status = sysctlbyname("sysctl.proc_translated", &translated, &length, NULL, 0);
    char brand[128] = "?";
    size_t blen = sizeof brand;
    sysctlbyname("machdep.cpu.brand_string", brand, &blen, NULL, 0);
    char vendor[64] = "?";
    size_t vlen = sizeof vendor;
    sysctlbyname("machdep.cpu.vendor", vendor, &vlen, NULL, 0);
    fprintf(stderr, "{\"sysctl_proc_translated\":%d,\"sysctl_status\":%d,\"cpu_brand\":\"%s\",\"cpu_vendor\":\"%s\"}\n", translated, status, brand, vendor);
    unsigned short cw = argc > 1 ? (unsigned short)strtoul(argv[1], 0, 16) : 0x007f;
    unsigned char in[21], out[22];
    while (fread(in, 1, 21, stdin) == 21) {
        unsigned char a[16] = {0}, b[16] = {0}, r0[16] = {0}, r1[16] = {0};
        unsigned short sw = 0;
        memcpy(a, in + 1, 10);
        memcpy(b, in + 11, 10);
        switch (in[0]) {
        case 0: __asm__ volatile("fninit\n\tfldcw %3\n\tfldt %2\n\tfsin\n\tfnstsw %1\n\tfstpt %0" : "=m"(r0), "=m"(sw) : "m"(a), "m"(cw) : "st"); break;
        case 1: __asm__ volatile("fninit\n\tfldcw %3\n\tfldt %2\n\tfcos\n\tfnstsw %1\n\tfstpt %0" : "=m"(r0), "=m"(sw) : "m"(a), "m"(cw) : "st"); break;
        case 2: __asm__ volatile("fninit\n\tfldcw %4\n\tfldt %3\n\tfldt %2\n\tfpatan\n\tfnstsw %1\n\tfstpt %0" : "=m"(r0), "=m"(sw) : "m"(a), "m"(b), "m"(cw) : "st", "st(1)"); break; /* b=y pushed first (ST1), a=x (ST0) */
        case 3: __asm__ volatile("fninit\n\tfldcw %3\n\tfldt %2\n\tf2xm1\n\tfnstsw %1\n\tfstpt %0" : "=m"(r0), "=m"(sw) : "m"(a), "m"(cw) : "st"); break;
        case 4: __asm__ volatile("fninit\n\tfldcw %4\n\tfldt %3\n\tfldt %2\n\tfyl2x\n\tfnstsw %1\n\tfstpt %0" : "=m"(r0), "=m"(sw) : "m"(a), "m"(b), "m"(cw) : "st", "st(1)"); break;
        case 5: __asm__ volatile("fninit\n\tfldcw %4\n\tfldt %3\n\tfldt %2\n\tfyl2xp1\n\tfnstsw %1\n\tfstpt %0" : "=m"(r0), "=m"(sw) : "m"(a), "m"(b), "m"(cw) : "st", "st(1)"); break;
        case 6: __asm__ volatile("fninit\n\tfldcw %4\n\tfldt %3\n\tfptan\n\tfnstsw %2\n\tfstpt %1\n\tfstpt %0" : "=m"(r0), "=m"(r1), "=m"(sw) : "m"(a), "m"(cw) : "st", "st(1)"); break;
        case 8: __asm__ volatile("fninit\n\tfldcw %4\n\tfldt %3\n\tfldt %2\n\tfscale\n\tfnstsw %1\n\tfstpt %0\n\tfstp %%st(0)" : "=m"(r0), "=m"(sw) : "m"(a), "m"(b), "m"(cw) : "st", "st(1)"); break; /* ST0 = a scaled by ST1 = b */
        case 9: __asm__ volatile("fninit\n\tfldcw %3\n\tfldt %2\n\tfrndint\n\tfnstsw %1\n\tfstpt %0" : "=m"(r0), "=m"(sw) : "m"(a), "m"(cw) : "st"); break;
        case 7: __asm__ volatile("fninit\n\tfldcw %4\n\tfldt %3\n\tfsincos\n\tfnstsw %2\n\tfstpt %1\n\tfstpt %0" : "=m"(r0), "=m"(r1), "=m"(sw) : "m"(a), "m"(cw) : "st", "st(1)"); break; /* ST0=cos popped first -> r1, sin -> r0 */
        }
        memcpy(out, r0, 10);
        memcpy(out + 10, r1, 10);
        memcpy(out + 20, &sw, 2);
        fwrite(out, 1, 22, stdout);
    }
    return ferror(stdin) || ferror(stdout);
}
