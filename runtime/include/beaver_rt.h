/* Beaver 0.1 runtime support library.
 *
 * Compiled together with every generated translation unit. All operations
 * that could invoke C undefined behaviour (signed overflow, out-of-bounds
 * access, division by zero, invalid conversions, allocation size overflow)
 * are checked here and terminate the process with a B5xxx diagnostic.
 */
#ifndef BEAVER_RT_H
#define BEAVER_RT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct bvr_loc {
    const char *file;
    int32_t line;
    int32_t col;
} bvr_loc;

/* Immutable UTF-8 string. `heap` strings own `ptr`; literals point to
 * static storage and are never freed. */
typedef struct bvr_string {
    uint8_t *ptr;
    int64_t len;
    bool heap;
} bvr_string;

/* Fixed-size, zero-initialized byte buffer. */
typedef struct bvr_buffer {
    uint8_t *ptr;
    int64_t len;
} bvr_buffer;

/* ----- process ----------------------------------------------------- */
void bvr_rt_init(int argc, char **argv);
int bvr_rt_finish(void);
_Noreturn void bvr_trap(const char *code, const char *msg, const bvr_loc *loc);
_Noreturn void bvr_unreachable(void);

/* ----- allocation (counted for leak / double-free detection) -------- */
void *bvr_alloc_array(int64_t elem_size, int64_t count, const bvr_loc *loc);
void *bvr_grow_array(void *ptr, int64_t elem_size, int64_t *cap, const bvr_loc *loc);
void bvr_free(void *ptr);
int64_t bvr_live_allocations(void);

/* ----- checked Int arithmetic --------------------------------------- */
int64_t bvr_add_i64(int64_t a, int64_t b, const bvr_loc *loc);
int64_t bvr_sub_i64(int64_t a, int64_t b, const bvr_loc *loc);
int64_t bvr_mul_i64(int64_t a, int64_t b, const bvr_loc *loc);
int64_t bvr_div_i64(int64_t a, int64_t b, const bvr_loc *loc);
int64_t bvr_rem_i64(int64_t a, int64_t b, const bvr_loc *loc);
int64_t bvr_neg_i64(int64_t a, const bvr_loc *loc);

/* ----- conversions -------------------------------------------------- */
int64_t bvr_f64_to_i64(double x, const bvr_loc *loc);
uint8_t bvr_i64_to_u8(int64_t x, const bvr_loc *loc);

/* ----- bounds ------------------------------------------------------- */
void bvr_bounds_check(int64_t index, int64_t len, const bvr_loc *loc);

/* ----- strings ------------------------------------------------------ */
bvr_string bvr_str_lit(const char *bytes, int64_t len);
bvr_string bvr_string_concat(const bvr_string *a, const bvr_string *b);
bvr_string bvr_string_clone(const bvr_string *s);
void bvr_string_drop(bvr_string *s);
bool bvr_string_eq(const bvr_string *a, const bvr_string *b);

/* ----- buffers ------------------------------------------------------ */
bvr_buffer bvr_buffer_new(int64_t size, const bvr_loc *loc);
bvr_buffer bvr_buffer_clone(const bvr_buffer *b);
void bvr_buffer_drop(bvr_buffer *b);
uint8_t *bvr_buffer_at(bvr_buffer *b, int64_t index, const bvr_loc *loc);

/* ----- output and assertions ---------------------------------------- */
void bvr_print_i64(int64_t v);
void bvr_print_f64(double v);
void bvr_print_bool(bool v);
void bvr_print_u8(uint8_t v);
void bvr_print_string(const bvr_string *s);
void bvr_assert(bool cond, const bvr_loc *loc);
void bvr_assert_eq_i64(int64_t a, int64_t b, const bvr_loc *loc);
void bvr_assert_eq_f64(double a, double b, const bvr_loc *loc);
void bvr_assert_eq_bool(bool a, bool b, const bvr_loc *loc);
void bvr_assert_eq_u8(uint8_t a, uint8_t b, const bvr_loc *loc);
void bvr_assert_eq_string(const bvr_string *a, const bvr_string *b, const bvr_loc *loc);
_Noreturn void bvr_assert_eq_failed(const bvr_loc *loc);
_Noreturn void bvr_unwrap_failed(const bvr_string *err, const bvr_loc *loc);
_Noreturn void bvr_unwrap_error_failed(const bvr_loc *loc);

/* ----- test driver -------------------------------------------------- */
typedef void (*bvr_test_fn)(void);
int bvr_test_main(int argc, char **argv, int count, const char *const *names, const bvr_test_fn *fns);

#endif
