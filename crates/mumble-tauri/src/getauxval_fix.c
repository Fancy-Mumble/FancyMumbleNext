/*
 * Safe getauxval replacement for Android aarch64.
 *
 * The NDK's libclang_rt.builtins ships outlined-atomics helpers whose
 * static constructor (init_have_lse_atomics) calls a statically-linked
 * getauxval.  That implementation dereferences a NULL pointer to the ELF
 * auxiliary vector when running inside a dlopen'd shared library,
 * crashing with SIGSEGV.
 *
 * This file provides a GLOBAL getauxval that reads /proc/self/auxv
 * directly.  Because the linker processes our object file before the
 * builtins archive, init_have_lse_atomics resolves to this safe version
 * instead of the broken one.
 */

#include <errno.h>
#include <fcntl.h>
#include <unistd.h>

/* ELF auxiliary vector entry (64-bit) */
typedef struct {
    unsigned long a_type;
    unsigned long a_val;
} auxv_t;

#define AT_NULL 0

__attribute__((visibility("default"), used))
unsigned long getauxval(unsigned long type) {
    int fd = open("/proc/self/auxv", O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        errno = ENOENT;
        return 0;
    }

    auxv_t entry;
    unsigned long result = 0;
    int found = 0;

    while (read(fd, &entry, sizeof(entry)) == (ssize_t)sizeof(entry)) {
        if (entry.a_type == AT_NULL)
            break;
        if (entry.a_type == type) {
            result = entry.a_val;
            found = 1;
            break;
        }
    }

    close(fd);

    if (!found)
        errno = ENOENT;

    return result;
}
