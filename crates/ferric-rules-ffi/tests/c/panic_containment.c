/*
 * Native C subprocess regression harness for generated panic-containment
 * wrappers. The linked Rust artifact must be built with the internal
 * test-only panic-injection build cfg.
 */

#define _POSIX_C_SOURCE 200809L

#include "ferric.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures = 0;

#define CHECK(condition, message)                                              \
    do {                                                                       \
        if (!(condition)) {                                                    \
            fprintf(stderr, "panic_containment: %s\n", (message));             \
            failures++;                                                        \
        }                                                                      \
    } while (0)

static void select_panic(const char *function_name) {
    CHECK(setenv("FERRIC_FFI_TEST_PANIC", function_name, 1) == 0,
          "failed to select test-only panic injection");
}

static void clear_panic(void) {
    CHECK(unsetenv("FERRIC_FFI_TEST_PANIC") == 0,
          "failed to clear test-only panic injection");
}

static void check_message(const char *message, const char *function_name,
                          const char *channel) {
    char expected[256];
    int written = snprintf(
        expected, sizeof(expected),
        "internal Rust panic contained in C ABI export `%s`", function_name);
    CHECK(written > 0 && (size_t)written < sizeof(expected),
          "expected diagnostic did not fit");
    if (written > 0 && (size_t)written < sizeof(expected)) {
        CHECK(message != NULL && strcmp(message, expected) == 0, channel);
    }
}

static void check_global_message(const char *function_name) {
    check_message(ferric_last_error_global(), function_name,
                  "global channel must contain exactly one stable panic report");
}

int main(void) {
    struct FerricEngine *raw = ferric_engine_new();
    CHECK(raw != NULL, "raw engine construction must succeed");
    if (raw == NULL) {
        return 1;
    }

    select_panic("ferric_engine_reset");
    enum FerricError status = ferric_engine_reset(raw);
    clear_panic();
    CHECK(status == FERRIC_ERROR_INTERNAL_ERROR,
          "status-returning export must use INTERNAL_ERROR sentinel");
    check_global_message("ferric_engine_reset");
    check_message(ferric_engine_last_error(raw), "ferric_engine_reset",
                  "raw-engine channel must mirror the panic report");
    CHECK(ferric_engine_reset(raw) == FERRIC_ERROR_OK,
          "raw engine must remain usable after a contained panic");

    select_panic("ferric_engine_new");
    struct FerricEngine *failed_engine = ferric_engine_new();
    clear_panic();
    CHECK(failed_engine == NULL,
          "pointer-returning export must use a NULL sentinel");
    check_global_message("ferric_engine_new");

    select_panic("ferric_value_integer");
    struct FerricValue value = ferric_value_integer(41);
    clear_panic();
    CHECK(value.value_type == FERRIC_VALUE_TYPE_VOID &&
              value.string_ptr == NULL && value.multifield_ptr == NULL,
          "FerricValue-returning export must use a Void sentinel");
    check_global_message("ferric_value_integer");

    select_panic("ferric_string_free");
    ferric_string_free(NULL);
    clear_panic();
    check_global_message("ferric_string_free");

    CHECK(ferric_engine_free(raw) == FERRIC_ERROR_OK,
          "raw engine cleanup must succeed");

    if (failures == 0) {
        puts("panic-containment harness: all return categories survived");
    }
    return failures == 0 ? 0 : 1;
}
