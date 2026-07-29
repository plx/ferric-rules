/*
 * output_lifetime.c — C-ABI regression harness for FR-CABI-004 (issue #114).
 *
 * A borrowed output pointer belongs to one engine. Reading the same channel
 * from a second engine must not invalidate the first engine's pointer. The
 * copy API must report its exact size and any truncation, and repeated engine
 * destruction must release engine-scoped borrowed storage.
 *
 * Run through `just ffi-c-harness`; the pre-fix process-wide channel cache
 * triggers an AddressSanitizer use-after-free when `first_output` is read
 * after the second engine replaces the shared cache entry.
 */

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "ferric.h"

static FerricEngine *engine_with_output(const char *channel, const char *text) {
    char source[256] = {0};
    uint64_t fired = 0;
    FerricEngine *engine = ferric_engine_new();
    if (engine == NULL) {
        return NULL;
    }

    int written = snprintf(
        source, sizeof(source),
        "(defrule emit => (printout %s \"%s\"))", channel, text);
    if (written < 0 || (size_t)written >= sizeof(source) ||
        ferric_engine_load_string(engine, source) != FERRIC_ERROR_OK ||
        ferric_engine_reset(engine) != FERRIC_ERROR_OK ||
        ferric_engine_run(engine, -1, &fired) != FERRIC_ERROR_OK ||
        fired != UINT64_C(1)) {
        (void)ferric_engine_free(engine);
        return NULL;
    }

    return engine;
}

static int check_copy_contract(FerricEngine *engine) {
    char small[5] = {'x', 'x', 'x', 'x', 'x'};
    char exact[32] = {0};
    uintptr_t needed = 0;

    if (ferric_engine_get_output_copy(engine, "shared", NULL, 0, &needed) !=
            FERRIC_ERROR_OK ||
        needed != sizeof("engine-a")) {
        fprintf(stderr, "output copy size query returned the wrong result\n");
        return 1;
    }

    uintptr_t reported = 0;
    if (ferric_engine_get_output_copy(engine, "shared", small, sizeof(small),
                                      &reported) !=
            FERRIC_ERROR_BUFFER_TOO_SMALL ||
        reported != needed || small[sizeof(small) - 1] != '\0' ||
        strcmp(small, "engi") != 0) {
        fprintf(stderr, "undersized output copy was not reported explicitly\n");
        return 1;
    }

    if (ferric_engine_get_output_copy(engine, "shared", exact, needed,
                                      &reported) != FERRIC_ERROR_OK ||
        reported != needed || strcmp(exact, "engine-a") != 0) {
        fprintf(stderr, "exact output copy returned the wrong result\n");
        return 1;
    }

    reported = UINTPTR_MAX;
    if (ferric_engine_get_output_copy(engine, "missing", NULL, 0, &reported) !=
            FERRIC_ERROR_NOT_FOUND ||
        reported != 0) {
        fprintf(stderr, "missing output copy returned the wrong result\n");
        return 1;
    }

    return 0;
}

static int stress_engine_lifecycle(void) {
    for (unsigned i = 0; i < 128; i++) {
        char channel[32] = {0};
        int written =
            snprintf(channel, sizeof(channel), "lifecycle_%03u", i);
        if (written < 0 || (size_t)written >= sizeof(channel)) {
            return 1;
        }

        FerricEngine *engine = engine_with_output(channel, "payload");
        if (engine == NULL) {
            fprintf(stderr, "lifecycle engine %u failed to initialize\n", i);
            return 1;
        }
        const char *output = ferric_engine_get_output(engine, channel);
        if (output == NULL || strcmp(output, "payload") != 0 ||
            ferric_engine_free(engine) != FERRIC_ERROR_OK) {
            fprintf(stderr, "lifecycle engine %u failed\n", i);
            return 1;
        }
    }
    return 0;
}

int main(void) {
    FerricEngine *first = engine_with_output("shared", "engine-a");
    FerricEngine *second = engine_with_output("shared", "engine-b");
    if (first == NULL || second == NULL) {
        fprintf(stderr, "failed to create output-producing engines\n");
        if (first != NULL) {
            (void)ferric_engine_free(first);
        }
        if (second != NULL) {
            (void)ferric_engine_free(second);
        }
        return 1;
    }

    const char *first_output = ferric_engine_get_output(first, "shared");
    if (first_output == NULL || strcmp(first_output, "engine-a") != 0) {
        fprintf(stderr, "first engine returned the wrong output\n");
        return 1;
    }

    const char *second_output = ferric_engine_get_output(second, "shared");
    if (second_output == NULL || strcmp(second_output, "engine-b") != 0) {
        fprintf(stderr, "second engine returned the wrong output\n");
        return 1;
    }

    if (strcmp(first_output, "engine-a") != 0) {
        fprintf(stderr, "second engine invalidated first engine's pointer\n");
        return 1;
    }
    if (check_copy_contract(first) != 0) {
        return 1;
    }

    if (ferric_engine_free(first) != FERRIC_ERROR_OK ||
        ferric_engine_free(second) != FERRIC_ERROR_OK) {
        fprintf(stderr, "failed to free output-producing engines\n");
        return 1;
    }
    if (stress_engine_lifecycle() != 0) {
        return 1;
    }

    printf("output-lifetime harness: pointer isolation, copy, and cleanup passed\n");
    return 0;
}
