/* Host test: emit one record of every kind with slrlog.h (argv[1] = output path). */
#include "../slrlog.h"

static void rec_native(slr_sink *s) {
    slr_rec r;
    slr_rec_init(&r);
    slr_put_cstr(&r, "java.game.GameType");
    slr_put_cstr(&r, "addCall");
    slr_put_u32(&r, 5);
    slr_val_int(&r, 1);
    slr_val_float_bits(&r, 0x40a00000u);   /* 5.0f */
    slr_val_float_bits(&r, 0x80000000u);   /* -0.0f survives */
    slr_val_cstr(&r, "caf\xe9");           /* Latin-1 e-acute widened to UTF-8 */
    slr_val_bool(&r, 1);
    slr_put_u8(&r, 1);                     /* result present */
    slr_val_long(&r, -2);
    slr_sink_commit(s, SLR_TAG_NATIVE, &r);
}
static void rec_family(slr_sink *s, uint8_t tag, uint8_t kind, const char *m) {
    slr_rec r;
    slr_rec_init(&r);
    slr_put_u8(&r, kind);
    slr_put_cstr(&r, m);
    slr_put_u32(&r, 2);
    slr_val_double_bits(&r, 0x3ff8000000000000ull); /* 1.5 */
    slr_val_bytes(&r, "\x01\x02\x03", 3);
    slr_sink_commit(s, tag, &r);
}
int main(int argc, char **argv) {
    slr_sink s;
    slr_rec r;
    if (argc < 2 || slr_sink_open(&s, argv[1])) return 2;
    rec_native(&s);
    rec_family(&s, SLR_TAG_D3D9, 0x01, "DrawIndexedPrimitive");
    rec_family(&s, SLR_TAG_DSOUND, 0x00, "Play");
    slr_rec_init(&r);
    slr_put_u8(&r, 0x00); /* keyboard */
    slr_put_u32(&r, 4);
    slr_put_raw(&r, "\x00\x01\x00\xff", 4);
    slr_sink_commit(&s, SLR_TAG_DINPUT, &r);
    slr_rec_init(&r);
    slr_put_u8(&r, 0x00); /* open */
    slr_put_cstr(&r, "sl/Scripts/game/Part.class");
    slr_put_u32(&r, 2);
    slr_val_cstr(&r, "r");
    slr_val_bool(&r, 1);
    slr_sink_commit(&s, SLR_TAG_FILE, &r);
    slr_rec_init(&r);
    slr_put_u64(&r, 7);   /* frame */
    slr_put_u32(&r, 0x401000);
    slr_put_u32(&r, 0x7f);
    slr_put_u32(&r, 1);
    slr_val_int(&r, 3);
    slr_sink_commit(&s, SLR_TAG_VMSNAP, &r);
    slr_rec_init(&r);
    slr_put_u64(&r, 7);
    { int i; for (i = 0; i < 9; i++) slr_put_u32(&r, 0x3f800000u + (uint32_t)i); }
    slr_put_u32(&r, 0x45fa0000u); /* rpm 8000.0 */
    slr_put_u32(&r, (uint32_t)-1);
    slr_put_u16(&r, 0);
    slr_sink_commit(&s, SLR_TAG_VEHICLE, &r);
    slr_sink_close(&s);
    return 0;
}
