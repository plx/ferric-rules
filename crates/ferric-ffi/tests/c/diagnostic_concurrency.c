/*
 * diagnostic_concurrency.c — ThreadSanitizer regression for FR-CABI-002.
 *
 * The creator thread repeatedly replaces and clears a raw engine's error
 * snapshot while foreign pthreads call both diagnostic accessors. Every owned
 * copy must be either absent or one complete published snapshot. Borrowed
 * pointers are deliberately not dereferenced because their public contract
 * requires external serialization after the call returns.
 *
 * Both this harness and the Rust static library must be instrumented with
 * ThreadSanitizer. Run through `just ffi-tsan-harness`.
 */

#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "ferric.h"

#define READER_COUNT 2
#define STRESS_ROUNDS 10000
#define SNAPSHOT_CAPACITY 512

typedef struct TestBarrier {
    pthread_mutex_t mutex;
    pthread_cond_t condition;
    unsigned int threshold;
    unsigned int waiting;
    unsigned int generation;
} TestBarrier;

typedef struct ReaderContext {
    const FerricEngine *engine;
    TestBarrier *barrier;
    const char *first;
    const char *second;
    int failures;
} ReaderContext;

static int barrier_init(TestBarrier *barrier, unsigned int threshold) {
    if (pthread_mutex_init(&barrier->mutex, NULL) != 0) {
        return 1;
    }
    if (pthread_cond_init(&barrier->condition, NULL) != 0) {
        (void)pthread_mutex_destroy(&barrier->mutex);
        return 1;
    }
    barrier->threshold = threshold;
    barrier->waiting = 0;
    barrier->generation = 0;
    return 0;
}

static void barrier_wait(TestBarrier *barrier) {
    (void)pthread_mutex_lock(&barrier->mutex);
    unsigned int generation = barrier->generation;
    barrier->waiting++;
    if (barrier->waiting == barrier->threshold) {
        barrier->waiting = 0;
        barrier->generation++;
        (void)pthread_cond_broadcast(&barrier->condition);
    } else {
        while (generation == barrier->generation) {
            (void)pthread_cond_wait(&barrier->condition, &barrier->mutex);
        }
    }
    (void)pthread_mutex_unlock(&barrier->mutex);
}

static void barrier_destroy(TestBarrier *barrier) {
    (void)pthread_cond_destroy(&barrier->condition);
    (void)pthread_mutex_destroy(&barrier->mutex);
}

static int copy_snapshot(const FerricEngine *engine, char *buffer,
                         uintptr_t capacity, uintptr_t *written) {
    FerricError result =
        ferric_engine_last_error_copy(engine, buffer, capacity, written);
    if (result != FERRIC_ERROR_OK) {
        fprintf(stderr, "diagnostic copy failed with code %d\n", (int)result);
        return 1;
    }
    if (*written < 2 || *written > capacity ||
        buffer[*written - 1] != '\0') {
        fprintf(stderr, "diagnostic copy returned an invalid length or NUL\n");
        return 1;
    }
    return 0;
}

static void *diagnostic_reader(void *opaque) {
    ReaderContext *context = (ReaderContext *)opaque;
    char buffer[SNAPSHOT_CAPACITY];

    for (int round = 0; round < STRESS_ROUNDS; round++) {
        barrier_wait(context->barrier);

        uintptr_t written = 0;
        FerricError result = ferric_engine_last_error_copy(
            context->engine, buffer, sizeof(buffer), &written);
        if (result == FERRIC_ERROR_OK) {
            if (written < 2 || written > sizeof(buffer) ||
                buffer[written - 1] != '\0') {
                fprintf(stderr,
                        "reader observed invalid diagnostic length or NUL\n");
                context->failures++;
            } else if (strcmp(buffer, context->first) != 0 &&
                       strcmp(buffer, context->second) != 0) {
                fprintf(stderr, "reader observed torn snapshot: %s\n", buffer);
                context->failures++;
            }
        } else if (result != FERRIC_ERROR_NOT_FOUND || written != 0) {
            fprintf(stderr, "reader copy failed with code %d and length %zu\n",
                    (int)result, (size_t)written);
            context->failures++;
        }

        /*
         * Exercise synchronization of the borrowed cache itself. Its returned
         * pointer is intentionally discarded before the next competing call.
         */
        (void)ferric_engine_last_error(context->engine);

        barrier_wait(context->barrier);
    }
    return NULL;
}

int main(void) {
    int failures = 0;
    FerricEngine *engine = ferric_engine_new();
    if (engine == NULL) {
        fprintf(stderr, "failed to create raw engine\n");
        return 1;
    }

    char first[SNAPSHOT_CAPACITY] = {0};
    char second[SNAPSHOT_CAPACITY] = {0};
    uintptr_t written = 0;

    if (ferric_engine_retract(engine, UINT64_MAX) !=
            FERRIC_ERROR_NOT_FOUND ||
        copy_snapshot(engine, first, sizeof(first), &written) != 0) {
        fprintf(stderr, "failed to capture first baseline diagnostic\n");
        failures++;
    }
    if (ferric_engine_retract(engine, UINT64_MAX - 1) !=
            FERRIC_ERROR_NOT_FOUND ||
        copy_snapshot(engine, second, sizeof(second), &written) != 0) {
        fprintf(stderr, "failed to capture second baseline diagnostic\n");
        failures++;
    }
    if (failures != 0) {
        (void)ferric_engine_free(engine);
        return 1;
    }
    if (strcmp(first, second) == 0) {
        fprintf(stderr, "baseline diagnostics must be distinct\n");
        (void)ferric_engine_free(engine);
        return 1;
    }

    TestBarrier barrier;
    if (barrier_init(&barrier, READER_COUNT + 1) != 0) {
        fprintf(stderr, "failed to initialize pthread barrier\n");
        (void)ferric_engine_free(engine);
        return 1;
    }

    pthread_t readers[READER_COUNT];
    ReaderContext contexts[READER_COUNT];
    for (int index = 0; index < READER_COUNT; index++) {
        contexts[index].engine = engine;
        contexts[index].barrier = &barrier;
        contexts[index].first = first;
        contexts[index].second = second;
        contexts[index].failures = 0;
        if (pthread_create(&readers[index], NULL, diagnostic_reader,
                           &contexts[index]) != 0) {
            fprintf(stderr, "failed to create diagnostic reader\n");
            return 1;
        }
    }

    for (int round = 0; round < STRESS_ROUNDS; round++) {
        barrier_wait(&barrier);

        FerricError result;
        if (round % 3 == 0) {
            result = ferric_engine_clear_error(engine);
            if (result != FERRIC_ERROR_OK) {
                fprintf(stderr, "owner clear failed with code %d\n",
                        (int)result);
                failures++;
            }
        } else {
            uint64_t fact_id = round % 2 == 0 ? UINT64_MAX : UINT64_MAX - 1;
            result = ferric_engine_retract(engine, fact_id);
            if (result != FERRIC_ERROR_NOT_FOUND) {
                fprintf(stderr, "owner mutation failed with code %d\n",
                        (int)result);
                failures++;
            }
        }

        barrier_wait(&barrier);
    }

    for (int index = 0; index < READER_COUNT; index++) {
        if (pthread_join(readers[index], NULL) != 0) {
            fprintf(stderr, "failed to join diagnostic reader\n");
            failures++;
        }
        failures += contexts[index].failures;
    }

    barrier_destroy(&barrier);
    if (ferric_engine_free(engine) != FERRIC_ERROR_OK) {
        fprintf(stderr, "failed to free raw engine\n");
        failures++;
    }

    if (failures != 0) {
        fprintf(stderr, "diagnostic concurrency harness: %d failure(s)\n",
                failures);
        return 1;
    }

    printf("diagnostic concurrency harness: %d owner operations and %d reader operations passed\n",
           STRESS_ROUNDS, STRESS_ROUNDS * READER_COUNT);
    return 0;
}
