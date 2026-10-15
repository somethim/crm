/* SQLite3 Multiple Ciphers 2.6.1 (SQLite 3.54.0) replacing stock SQLite. */
#ifndef SQLITE_TEMP_STORE
#define SQLITE_TEMP_STORE 2        /* temp tables/indices in memory, never plaintext on disk */
#endif
#define SQLITE_SECURE_DELETE 1
#define SQLITE3MC_SECURE_MEMORY 1  /* wipe freed memory */
#define CODEC_TYPE CODEC_TYPE_CHACHA20
#define HAVE_CIPHER_AES_128_CBC 0
#define HAVE_CIPHER_AES_256_CBC 0
#define HAVE_CIPHER_SQLCIPHER 0
#define HAVE_CIPHER_RC4 0
#define HAVE_CIPHER_ASCON128 0
#define HAVE_CIPHER_AEGIS 0
/* clang-cl (cargo-xwin) defines both _MSC_VER and __SIZEOF_INT128__: sqlite3mc 2.6.1
   then takes the 128-bit poly1305 path but skips both SODIUM_* macro sets. Supply them. */
#if defined(__clang__) && defined(_MSC_VER) && defined(__SIZEOF_INT128__)
#define SODIUM_MUL(out, x, y) out = ((unsigned __int128) x * y)
#define SODIUM_ADD(out, in) out += in
#define SODIUM_ADDLO(out, in) out += in
#define SODIUM_SHR(in, shift) (unsigned long long) (in >> (shift))
#define SODIUM_LO(in) (unsigned long long) (in)
#endif
#include "sqlite3mc_amalgamation.c"
