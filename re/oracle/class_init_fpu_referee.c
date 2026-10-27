// Independent native x87 FILD int32 -> FSTP float32, including exception/C1.
// The source417e29/417e32 instructions use this exact conversion sequence.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
int main(int argc,char **argv){
 if(argc!=3)return 2;
 uint16_t cw=(uint16_t)strtoul(argv[1],NULL,0),status;
 int32_t value=(int32_t)strtoll(argv[2],NULL,0);
 union {float value;uint32_t bits;} result;
 __asm__ volatile("fninit\n\tfldcw %[cw]\n\tfildl %[value]\n\tfstps %[result]\n\tfnstsw %[status]\n\tfninit"
 :[result]"=m"(result.value),[status]"=m"(status):[cw]"m"(cw),[value]"m"(value):"st","memory");
 printf("{\"bits\":%u,\"status\":%u}\n",result.bits,status);return 0;
}
