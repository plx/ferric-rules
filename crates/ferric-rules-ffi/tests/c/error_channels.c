/*
 * error_channels.c — C-ABI regression harness for FR-CABI-003 (issue #113).
 *
 * Valid-engine failures must publish the same current text to the engine
 * snapshot and the thread-local global fallback. Pre-handle failures can
 * update only the global channel. Interleaved engines must remain isolated.
 *
 * Run through `just ffi-c-harness`.
 */

#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "ferric.h"

#define ERROR_CAPACITY 512

static int copy_engine_error(const FerricEngine *engine, char *buffer,
                             uintptr_t capacity) {
    uintptr_t written = 0;
    FerricError result =
        ferric_engine_last_error_copy(engine, buffer, capacity, &written);
    if (result != FERRIC_ERROR_OK || written < 2 || written > capacity ||
        buffer[written - 1] != '\0') {
        fprintf(stderr,
                "engine error copy failed: code=%d written=%" PRIuPTR "\n",
                (int)result, written);
        return 1;
    }
    return 0;
}

static int copy_global_error(char *buffer, uintptr_t capacity) {
    uintptr_t written = 0;
    FerricError result =
        ferric_last_error_global_copy(buffer, capacity, &written);
    if (result != FERRIC_ERROR_OK || written < 2 || written > capacity ||
        buffer[written - 1] != '\0') {
        fprintf(stderr,
                "global error copy failed: code=%d written=%" PRIuPTR "\n",
                (int)result, written);
        return 1;
    }
    return 0;
}

static int expect_engine_error(const FerricEngine *engine,
                               const char *expected) {
    char actual[ERROR_CAPACITY] = {0};
    if (copy_engine_error(engine, actual, sizeof(actual)) != 0) {
        return 1;
    }
    if (strcmp(actual, expected) != 0) {
        fprintf(stderr, "engine error mismatch:\n  got:  %s\n  want: %s\n",
                actual, expected);
        return 1;
    }
    return 0;
}

static int expect_global_error(const char *expected) {
    char actual[ERROR_CAPACITY] = {0};
    if (copy_global_error(actual, sizeof(actual)) != 0) {
        return 1;
    }
    if (strcmp(actual, expected) != 0) {
        fprintf(stderr, "global error mismatch:\n  got:  %s\n  want: %s\n",
                actual, expected);
        return 1;
    }
    return 0;
}

static int expect_current_error(const FerricEngine *engine,
                                const char *expected) {
    return expect_engine_error(engine, expected) +
           expect_global_error(expected);
}

int main(void) {
    int failures = 0;
    FerricEngine *first = ferric_engine_new();
    FerricEngine *second = ferric_engine_new();
    if (first == NULL || second == NULL) {
        fprintf(stderr, "failed to create engines\n");
        return 1;
    }

    if (ferric_engine_load_string(first, "(defrule stale-parse") ==
        FERRIC_ERROR_OK) {
        fprintf(stderr, "invalid source unexpectedly loaded\n");
        failures++;
    }

    uintptr_t field_count = 0;
    char first_missing[ERROR_CAPACITY] = {0};
    char second_missing[ERROR_CAPACITY] = {0};
    (void)snprintf(first_missing, sizeof(first_missing),
                   "fact not found: %" PRIu64, UINT64_MAX);
    (void)snprintf(second_missing, sizeof(second_missing),
                   "fact not found: %" PRIu64, UINT64_MAX - UINT64_C(1));

    if (ferric_engine_get_fact_field_count(first, UINT64_MAX, &field_count) !=
        FERRIC_ERROR_NOT_FOUND) {
        fprintf(stderr, "first missing-fact query returned the wrong code\n");
        failures++;
    }
    failures += expect_current_error(first, first_missing);

    if (ferric_engine_get_fact_field_count(
            second, UINT64_MAX - UINT64_C(1), &field_count) !=
        FERRIC_ERROR_NOT_FOUND) {
        fprintf(stderr, "second missing-fact query returned the wrong code\n");
        failures++;
    }
    failures += expect_current_error(second, second_missing);
    failures += expect_engine_error(first, first_missing);

    if (ferric_engine_fact_count(first, NULL) != FERRIC_ERROR_NULL_POINTER) {
        fprintf(stderr, "valid-engine null output returned the wrong code\n");
        failures++;
    }
    failures += expect_current_error(first, "out_count pointer is null");
    failures += expect_engine_error(second, second_missing);

    if (ferric_engine_fact_count(NULL, &field_count) !=
        FERRIC_ERROR_NULL_POINTER) {
        fprintf(stderr, "null engine returned the wrong code\n");
        failures++;
    }
    failures += expect_global_error("engine pointer is null");
    failures += expect_engine_error(first, "out_count pointer is null");
    failures += expect_engine_error(second, second_missing);

    if (ferric_engine_free(first) != FERRIC_ERROR_OK ||
        ferric_engine_free(second) != FERRIC_ERROR_OK) {
        fprintf(stderr, "failed to free engines\n");
        failures++;
    }

    if (failures != 0) {
        fprintf(stderr, "error-channel harness: %d failure(s)\n", failures);
        return 1;
    }

    printf("error-channel harness: sequence, isolation, and null-handle checks passed\n");
    return 0;
}
