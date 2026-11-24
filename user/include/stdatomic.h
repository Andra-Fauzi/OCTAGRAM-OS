#ifndef OCTAGRAM_STDATOMIC_H
#define OCTAGRAM_STDATOMIC_H

/* CPython dan banyak kode modern memakai <stdatomic.h>; GCC sudah
   menyediakan implementasinya, jadi cukup diteruskan.

   Di C++ header bawaan GCC memakai `_Atomic`, yang bukan keyword C++ dan
   gagal dikompilasi. Kode C++ memakai <atomic> atau builtin __atomic_*, jadi
   header ini sengaja kosong untuk C++. */
#ifndef __cplusplus
#include_next <stdatomic.h>
#endif

#endif
