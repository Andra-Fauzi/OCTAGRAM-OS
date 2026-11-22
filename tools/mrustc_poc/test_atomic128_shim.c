#include <stddef.h>
#include <stdint.h>

#include "fix.h"

typedef struct {
    unsigned char bytes[16];
} octagram_u128_blob;

typedef struct {
    octagram_u128_blob _0;
} octagram_expected_holder;

/* Mirror the call shape produced by patch_core.py for mrustc's C output. */
static int generated_atomic128_call(octagram_u128_blob *arg0,
                                    octagram_expected_holder var1,
                                    octagram_u128_blob arg2)
{
    return poc_cxchg128((void *)arg0, (void *)&var1._0,
                        (const void *)&arg2);
}

static int bytes_equal(const unsigned char *left,
                       const unsigned char *right,
                       size_t size)
{
    size_t index;
    for (index = 0; index < size; ++index) {
        if (left[index] != right[index])
            return 0;
    }
    return 1;
}

int main(void)
{
    unsigned char object[16];
    unsigned char expected[16];
    unsigned char desired[16];
    unsigned char original[16];
    size_t index;

    for (index = 0; index < 16; ++index) {
        object[index] = (unsigned char)index;
        expected[index] = (unsigned char)index;
        original[index] = (unsigned char)index;
        desired[index] = (unsigned char)(0xa0u + index);
    }

    if (!poc_cxchg128(object, expected, desired))
        return 1;
    if (!bytes_equal(object, desired, 16))
        return 2;
    if (!bytes_equal(expected, original, 16))
        return 3;

    for (index = 0; index < 16; ++index) {
        object[index] = (unsigned char)(0x40u + index);
        expected[index] = (unsigned char)index;
    }
    if (poc_cxchg128(object, expected, desired))
        return 4;
    if (!bytes_equal(object, (const unsigned char[]){
            0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
            0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f
        }, 16))
        return 5;
    if (!bytes_equal(expected, object, 16))
        return 6;

    {
        octagram_u128_blob object_blob;
        octagram_u128_blob desired_blob;
        octagram_expected_holder holder;
        for (index = 0; index < 16; ++index) {
            object_blob.bytes[index] = (unsigned char)index;
            holder._0.bytes[index] = (unsigned char)index;
            desired_blob.bytes[index] = (unsigned char)(0xf0u - index);
        }
        if (!generated_atomic128_call(&object_blob, holder, desired_blob))
            return 7;
        if (!bytes_equal(object_blob.bytes, desired_blob.bytes, 16))
            return 8;
    }

    return 0;
}
