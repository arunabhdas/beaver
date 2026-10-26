/* Beaver 0.1 runtime support library. See beaver_rt.h. */
#include "beaver_rt.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Use compiler overflow builtins when available unless BVR_NO_BUILTINS is
 * defined; the portable C11 fallback is exercised by the test-suite. */
#if !defined(BVR_NO_BUILTINS) && defined(__has_builtin)
#if __has_builtin(__builtin_add_overflow) && __has_builtin(__builtin_sub_overflow) && \
    __has_builtin(__builtin_mul_overflow)
#define BVR_HAVE_OVERFLOW_BUILTINS 1
#endif
#endif

static int64_t bvr_allocs_live;
static bool bvr_leak_check;

void bvr_rt_init(int argc, char **argv) {
    (void)argc;
    (void)argv;
    const char *v = getenv("BEAVER_RT_LEAK_CHECK");
    bvr_leak_check = v != NULL && strcmp(v, "1") == 0;
}

int bvr_rt_finish(void) {
    fflush(stdout);
    if (bvr_leak_check) {
        if (bvr_allocs_live != 0) {
            fprintf(stderr, "runtime error[B5099]: %lld owned allocation(s) were not released\n",
                    (long long)bvr_allocs_live);
            return 102;
        }
        fprintf(stderr, "beaver-rt: all owned allocations released\n");
    }
    return 0;
}

_Noreturn void bvr_trap(const char *code, const char *msg, const bvr_loc *loc) {
    fflush(stdout);
    fprintf(stderr, "runtime error[%s]: %s\n", code, msg);
    if (loc != NULL && loc->file != NULL) {
        fprintf(stderr, "  --> %s:%d:%d\n", loc->file, (int)loc->line, (int)loc->col);
    }
    fflush(stderr);
    exit(101);
}

_Noreturn void bvr_unreachable(void) {
    bvr_trap("B5000", "internal error: reached unreachable code", NULL);
}

/* ----- allocation ------------------------------------------------- */

static size_t bvr_checked_bytes(int64_t elem_size, int64_t count, const bvr_loc *loc) {
    if (elem_size <= 0 || count < 0) {
        bvr_trap("B5007", "invalid allocation size", loc);
    }
    if (count > INT64_MAX / elem_size) {
        bvr_trap("B5007", "allocation size overflows", loc);
    }
    int64_t bytes = elem_size * count;
    if ((uint64_t)bytes > (uint64_t)SIZE_MAX) {
        bvr_trap("B5007", "allocation is too large for this platform", loc);
    }
    return (size_t)bytes;
}

void *bvr_alloc_array(int64_t elem_size, int64_t count, const bvr_loc *loc) {
    size_t bytes = bvr_checked_bytes(elem_size, count, loc);
    void *p = calloc(bytes == 0 ? 1 : bytes, 1);
    if (p == NULL) {
        bvr_trap("B5007", "out of memory", loc);
    }
    bvr_allocs_live++;
    return p;
}

void *bvr_grow_array(void *ptr, int64_t elem_size, int64_t *cap, const bvr_loc *loc) {
    int64_t old = *cap;
    int64_t next;
    if (old == 0) {
        next = 4;
    } else {
        if (old > INT64_MAX / 2) {
            bvr_trap("B5007", "list capacity overflows", loc);
        }
        next = old * 2;
    }
    size_t bytes = bvr_checked_bytes(elem_size, next, loc);
    void *p = realloc(ptr, bytes);
    if (p == NULL) {
        bvr_trap("B5007", "out of memory", loc);
    }
    if (ptr == NULL) {
        bvr_allocs_live++;
    }
    *cap = next;
    return p;
}

void bvr_free(void *ptr) {
    if (ptr == NULL) {
        return;
    }
    bvr_allocs_live--;
    if (bvr_allocs_live < 0) {
        bvr_trap("B5098", "internal error: owned allocation released twice", NULL);
    }
    free(ptr);
}

int64_t bvr_live_allocations(void) {
    return bvr_allocs_live;
}

/* ----- checked arithmetic ----------------------------------------- */

static _Noreturn void bvr_overflow(const char *op, const bvr_loc *loc) {
    char msg[96];
    snprintf(msg, sizeof msg, "integer overflow in %s", op);
    bvr_trap("B5001", msg, loc);
}

int64_t bvr_add_i64(int64_t a, int64_t b, const bvr_loc *loc) {
#ifdef BVR_HAVE_OVERFLOW_BUILTINS
    int64_t r;
    if (__builtin_add_overflow(a, b, &r)) {
        bvr_overflow("addition", loc);
    }
    return r;
#else
    if ((b > 0 && a > INT64_MAX - b) || (b < 0 && a < INT64_MIN - b)) {
        bvr_overflow("addition", loc);
    }
    return a + b;
#endif
}

int64_t bvr_sub_i64(int64_t a, int64_t b, const bvr_loc *loc) {
#ifdef BVR_HAVE_OVERFLOW_BUILTINS
    int64_t r;
    if (__builtin_sub_overflow(a, b, &r)) {
        bvr_overflow("subtraction", loc);
    }
    return r;
#else
    if ((b < 0 && a > INT64_MAX + b) || (b > 0 && a < INT64_MIN + b)) {
        bvr_overflow("subtraction", loc);
    }
    return a - b;
#endif
}

int64_t bvr_mul_i64(int64_t a, int64_t b, const bvr_loc *loc) {
#ifdef BVR_HAVE_OVERFLOW_BUILTINS
    int64_t r;
    if (__builtin_mul_overflow(a, b, &r)) {
        bvr_overflow("multiplication", loc);
    }
    return r;
#else
    if (a > 0) {
        if (b > 0) {
            if (a > INT64_MAX / b) {
                bvr_overflow("multiplication", loc);
            }
        } else if (b < INT64_MIN / a) {
            bvr_overflow("multiplication", loc);
        }
    } else if (a < 0) {
        if (b > 0) {
            if (a < INT64_MIN / b) {
                bvr_overflow("multiplication", loc);
            }
        } else if (b < 0 && a < INT64_MAX / b) {
            bvr_overflow("multiplication", loc);
        }
    }
    return a * b;
#endif
}

int64_t bvr_div_i64(int64_t a, int64_t b, const bvr_loc *loc) {
    if (b == 0) {
        bvr_trap("B5002", "division by zero", loc);
    }
    if (a == INT64_MIN && b == -1) {
        bvr_overflow("division", loc);
    }
    return a / b;
}

int64_t bvr_rem_i64(int64_t a, int64_t b, const bvr_loc *loc) {
    if (b == 0) {
        bvr_trap("B5002", "remainder by zero", loc);
    }
    if (b == -1) {
        return 0; /* avoids INT64_MIN % -1, which is undefined in C */
    }
    return a % b;
}

int64_t bvr_neg_i64(int64_t a, const bvr_loc *loc) {
    if (a == INT64_MIN) {
        bvr_overflow("negation", loc);
    }
    return -a;
}

/* ----- conversions ------------------------------------------------ */

int64_t bvr_f64_to_i64(double x, const bvr_loc *loc) {
    /* -2^63 is exactly representable; 2^63 is the first value out of range.
     * NaN fails both comparisons. */
    if (!(x >= -9223372036854775808.0 && x < 9223372036854775808.0)) {
        bvr_trap("B5004", "Float value is NaN, infinite or out of Int range", loc);
    }
    return (int64_t)x;
}

uint8_t bvr_i64_to_u8(int64_t x, const bvr_loc *loc) {
    if (x < 0 || x > 255) {
        char msg[96];
        snprintf(msg, sizeof msg, "Byte(%lld) is out of range 0..255", (long long)x);
        bvr_trap("B5004", msg, loc);
    }
    return (uint8_t)x;
}

/* ----- bounds ----------------------------------------------------- */

void bvr_bounds_check(int64_t index, int64_t len, const bvr_loc *loc) {
    if (index < 0 || index >= len) {
        char msg[128];
        snprintf(msg, sizeof msg, "index %lld is out of bounds for length %lld", (long long)index,
                 (long long)len);
        bvr_trap("B5003", msg, loc);
    }
}

/* ----- strings ---------------------------------------------------- */

bvr_string bvr_str_lit(const char *bytes, int64_t len) {
    bvr_string s;
    s.ptr = (uint8_t *)(uintptr_t)bytes;
    s.len = len;
    s.heap = false;
    return s;
}

static bvr_string bvr_string_alloc(int64_t len) {
    bvr_string s;
    s.ptr = (uint8_t *)bvr_alloc_array(1, len, NULL);
    s.len = len;
    s.heap = true;
    return s;
}

bvr_string bvr_string_concat(const bvr_string *a, const bvr_string *b) {
    if (a->len > INT64_MAX - b->len) {
        bvr_trap("B5007", "string length overflows", NULL);
    }
    bvr_string s = bvr_string_alloc(a->len + b->len);
    if (a->len > 0) {
        memcpy(s.ptr, a->ptr, (size_t)a->len);
    }
    if (b->len > 0) {
        memcpy(s.ptr + a->len, b->ptr, (size_t)b->len);
    }
    return s;
}

bvr_string bvr_string_clone(const bvr_string *src) {
    bvr_string s = bvr_string_alloc(src->len);
    if (src->len > 0) {
        memcpy(s.ptr, src->ptr, (size_t)src->len);
    }
    return s;
}

void bvr_string_drop(bvr_string *s) {
    if (s->heap) {
        bvr_free(s->ptr);
    }
    s->ptr = NULL;
    s->len = 0;
    s->heap = false;
}

bool bvr_string_eq(const bvr_string *a, const bvr_string *b) {
    if (a->len != b->len) {
        return false;
    }
    return a->len == 0 || memcmp(a->ptr, b->ptr, (size_t)a->len) == 0;
}

/* ----- buffers ---------------------------------------------------- */

bvr_buffer bvr_buffer_new(int64_t size, const bvr_loc *loc) {
    if (size < 0) {
        char msg[96];
        snprintf(msg, sizeof msg, "Buffer size %lld is negative", (long long)size);
        bvr_trap("B5008", msg, loc);
    }
    bvr_buffer b;
    b.ptr = (uint8_t *)bvr_alloc_array(1, size, loc);
    b.len = size;
    return b;
}

bvr_buffer bvr_buffer_clone(const bvr_buffer *src) {
    bvr_buffer b;
    b.ptr = (uint8_t *)bvr_alloc_array(1, src->len, NULL);
    b.len = src->len;
    if (src->len > 0) {
        memcpy(b.ptr, src->ptr, (size_t)src->len);
    }
    return b;
}

void bvr_buffer_drop(bvr_buffer *b) {
    bvr_free(b->ptr);
    b->ptr = NULL;
    b->len = 0;
}

uint8_t *bvr_buffer_at(bvr_buffer *b, int64_t index, const bvr_loc *loc) {
    bvr_bounds_check(index, b->len, loc);
    return &b->ptr[index];
}

/* ----- output ----------------------------------------------------- */

void bvr_print_i64(int64_t v) {
    printf("%lld\n", (long long)v);
}

static void bvr_format_f64(double v, char *buf, size_t n) {
    /* Shortest representation that round-trips, always with a decimal
     * point or exponent so Floats are distinguishable from Ints. */
    for (int prec = 1; prec <= 17; prec++) {
        snprintf(buf, n, "%.*g", prec, v);
        if (strtod(buf, NULL) == v) {
            break;
        }
    }
    if (strpbrk(buf, ".eEn") == NULL) {
        size_t len = strlen(buf);
        if (len + 2 < n) {
            buf[len] = '.';
            buf[len + 1] = '0';
            buf[len + 2] = '\0';
        }
    }
}

void bvr_print_f64(double v) {
    char buf[64];
    bvr_format_f64(v, buf, sizeof buf);
    printf("%s\n", buf);
}

void bvr_print_bool(bool v) {
    printf("%s\n", v ? "true" : "false");
}

void bvr_print_u8(uint8_t v) {
    printf("%u\n", (unsigned)v);
}

void bvr_print_string(const bvr_string *s) {
    if (s->len > 0) {
        fwrite(s->ptr, 1, (size_t)s->len, stdout);
    }
    fputc('\n', stdout);
}

/* ----- assertions ------------------------------------------------- */

void bvr_assert(bool cond, const bvr_loc *loc) {
    if (!cond) {
        bvr_trap("B5005", "assertion failed", loc);
    }
}

_Noreturn void bvr_assert_eq_failed(const bvr_loc *loc) {
    bvr_trap("B5005", "assert_eq failed: values are not equal", loc);
}

void bvr_assert_eq_i64(int64_t a, int64_t b, const bvr_loc *loc) {
    if (a != b) {
        char msg[160];
        snprintf(msg, sizeof msg, "assert_eq failed: left = %lld, right = %lld", (long long)a,
                 (long long)b);
        bvr_trap("B5005", msg, loc);
    }
}

void bvr_assert_eq_f64(double a, double b, const bvr_loc *loc) {
    if (!(a == b)) {
        char x[64], y[64], msg[192];
        bvr_format_f64(a, x, sizeof x);
        bvr_format_f64(b, y, sizeof y);
        snprintf(msg, sizeof msg, "assert_eq failed: left = %s, right = %s", x, y);
        bvr_trap("B5005", msg, loc);
    }
}

void bvr_assert_eq_bool(bool a, bool b, const bvr_loc *loc) {
    if (a != b) {
        char msg[96];
        snprintf(msg, sizeof msg, "assert_eq failed: left = %s, right = %s", a ? "true" : "false",
                 b ? "true" : "false");
        bvr_trap("B5005", msg, loc);
    }
}

void bvr_assert_eq_u8(uint8_t a, uint8_t b, const bvr_loc *loc) {
    if (a != b) {
        char msg[96];
        snprintf(msg, sizeof msg, "assert_eq failed: left = %u, right = %u", (unsigned)a, (unsigned)b);
        bvr_trap("B5005", msg, loc);
    }
}

static void bvr_quote(const bvr_string *s, char *out, size_t n) {
    size_t j = 0;
    int64_t limit = s->len > 60 ? 60 : s->len;
    out[j++] = '"';
    for (int64_t i = 0; i < limit && j + 5 < n; i++) {
        uint8_t c = s->ptr[i];
        if (c == '"' || c == '\\') {
            out[j++] = '\\';
            out[j++] = (char)c;
        } else if (c >= 0x20 && c != 0x7f) {
            out[j++] = (char)c;
        } else {
            j += (size_t)snprintf(out + j, n - j, "\\x%02x", (unsigned)c);
        }
    }
    if (s->len > limit && j + 4 < n) {
        out[j++] = '.';
        out[j++] = '.';
        out[j++] = '.';
    }
    if (j + 1 < n) {
        out[j++] = '"';
    }
    out[j < n ? j : n - 1] = '\0';
}

void bvr_assert_eq_string(const bvr_string *a, const bvr_string *b, const bvr_loc *loc) {
    if (!bvr_string_eq(a, b)) {
        char x[300], y[300], msg[700];
        bvr_quote(a, x, sizeof x);
        bvr_quote(b, y, sizeof y);
        snprintf(msg, sizeof msg, "assert_eq failed: left = %s, right = %s", x, y);
        bvr_trap("B5005", msg, loc);
    }
}

_Noreturn void bvr_unwrap_failed(const bvr_string *err, const bvr_loc *loc) {
    char q[300], msg[400];
    bvr_quote(err, q, sizeof q);
    snprintf(msg, sizeof msg, "unwrap() called on a failed Result: %s", q);
    bvr_trap("B5006", msg, loc);
}

_Noreturn void bvr_unwrap_error_failed(const bvr_loc *loc) {
    bvr_trap("B5006", "unwrap_error() called on a successful Result", loc);
}

/* ----- tests ------------------------------------------------------ */

int bvr_test_main(int argc, char **argv, int count, const char *const *names, const bvr_test_fn *fns) {
    if (argc == 3 && strcmp(argv[1], "--run") == 0) {
        char *end = NULL;
        long i = strtol(argv[2], &end, 10);
        if (end == argv[2] || *end != '\0' || i < 0 || i >= count) {
            fprintf(stderr, "beaver test driver: invalid test index\n");
            return 2;
        }
        fns[i]();
        return bvr_rt_finish();
    }
    if (argc == 2 && strcmp(argv[1], "--count") == 0) {
        printf("%d\n", count);
        return 0;
    }
    /* In-process fallback: a trap stops the whole run. */
    for (int i = 0; i < count; i++) {
        printf("test %s ... ", names[i]);
        fflush(stdout);
        fns[i]();
        printf("ok\n");
    }
    return bvr_rt_finish();
}
