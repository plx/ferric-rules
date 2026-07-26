/*
 * multifield_copy.c — C-ABI regression harness for FR-CABI-008 (issue #87).
 *
 * Exercises ferric_value_multifield_copy with genuinely foreign input:
 * stack arrays, C malloc arrays, and borrowed string storage. The constructor
 * must deep-copy the complete nested tree, retain only shallow external-address
 * payload pointers, and return a tree that ferric_value_free can release using
 * Ferric's allocator provenance.
 *
 * Intended to run under AddressSanitizer + UndefinedBehaviorSanitizer via
 * `just ffi-c-harness`. Repeated late failures exercise partial-allocation
 * cleanup so LeakSanitizer, where supported (including Linux CI), detects any
 * abandoned Ferric-owned values.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "ferric.h"

static int failures = 0;

#define CHECK(cond, msg)                                              \
    do {                                                              \
        if (!(cond)) {                                                \
            fprintf(stderr, "FAIL(%s:%d): %s\n", __FILE__, __LINE__,  \
                    (msg));                                           \
            failures++;                                               \
        }                                                             \
    } while (0)

static FerricValue borrowed_text(uint32_t value_type, char *text) {
    FerricValue value = ferric_value_void();
    value.value_type = value_type;
    value.string_ptr = text;
    return value;
}

static void check_void(const FerricValue *value, const char *context) {
    CHECK(value->value_type == FERRIC_VALUE_TYPE_VOID, context);
    CHECK(value->integer == 0, context);
    CHECK(value->float_ == 0.0, context);
    CHECK(value->string_ptr == NULL, context);
    CHECK(value->multifield_ptr == NULL, context);
    CHECK(value->multifield_len == 0, context);
    CHECK(value->external_type_id == 0, context);
    CHECK(value->external_pointer == NULL, context);
}

static void test_empty_and_error_contract(void) {
    FerricValue out = ferric_value_integer(91);
    FerricError err = ferric_value_multifield_copy(NULL, 0, &out);
    CHECK(err == FERRIC_ERROR_OK, "NULL, 0 must construct an empty multifield");
    CHECK(out.value_type == FERRIC_VALUE_TYPE_MULTIFIELD,
          "empty copy must return a multifield");
    CHECK(out.multifield_ptr == NULL && out.multifield_len == 0,
          "empty copy must use a null, zero-length array");
    CHECK(ferric_value_free(&out) == FERRIC_ERROR_OK,
          "empty Ferric-owned multifield must be freeable");

    CHECK(ferric_value_multifield_copy(NULL, 0, NULL) ==
              FERRIC_ERROR_NULL_POINTER,
          "null output must return NULL_POINTER");

    out = ferric_value_integer(92);
    err = ferric_value_multifield_copy(NULL, 1, &out);
    CHECK(err == FERRIC_ERROR_NULL_POINTER,
          "NULL with non-zero length must return NULL_POINTER");
    check_void(&out, "NULL with non-zero length must leave output Void");

    FerricValue bad = ferric_value_void();
    bad.value_type = 0xDEADBEEFu;
    bad.string_ptr = (char *)&bad;
    bad.multifield_ptr = &bad;
    bad.multifield_len = SIZE_MAX;
    out = ferric_value_integer(93);
    err = ferric_value_multifield_copy(&bad, 1, &out);
    CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
          "unknown nested tags must return INVALID_ARGUMENT");
    check_void(&out, "unknown nested tags must leave output Void");

    FerricValue null_string = ferric_value_void();
    null_string.value_type = FERRIC_VALUE_TYPE_STRING;
    out = ferric_value_integer(94);
    err = ferric_value_multifield_copy(&null_string, 1, &out);
    CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
          "null active string pointer must return INVALID_ARGUMENT");
    check_void(&out, "null active string pointer must leave output Void");

    FerricValue null_nested = ferric_value_void();
    null_nested.value_type = FERRIC_VALUE_TYPE_MULTIFIELD;
    null_nested.multifield_len = 1;
    out = ferric_value_integer(95);
    err = ferric_value_multifield_copy(&null_nested, 1, &out);
    CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
          "null non-empty nested array must return INVALID_ARGUMENT");
    check_void(&out, "null non-empty nested array must leave output Void");

    FerricValue cycle = ferric_value_void();
    cycle.value_type = FERRIC_VALUE_TYPE_MULTIFIELD;
    cycle.multifield_ptr = &cycle;
    cycle.multifield_len = 1;
    out = ferric_value_integer(97);
    err = ferric_value_multifield_copy(&cycle, 1, &out);
    CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
          "cyclic input must return INVALID_ARGUMENT");
    check_void(&out, "cyclic input must leave output Void");

    FerricValue overlap[2];
    overlap[0] = ferric_value_void();
    overlap[0].value_type = FERRIC_VALUE_TYPE_MULTIFIELD;
    overlap[0].multifield_ptr = &overlap[1];
    overlap[0].multifield_len = 1;
    overlap[1] = ferric_value_integer(9);
    out = ferric_value_integer(98);
    err = ferric_value_multifield_copy(overlap, 2, &out);
    CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
          "ancestor-overlapping input must return INVALID_ARGUMENT");
    check_void(&out, "ancestor-overlapping input must leave output Void");
}

static void test_stack_and_malloc_tree(void) {
    char top_symbol[] = "alpha";
    char top_string[] = "hello";
    char *nested_symbol = (char *)malloc(sizeof("nested"));
    int external_payload = 77;

    CHECK(nested_symbol != NULL, "nested C string allocation must succeed");
    if (nested_symbol == NULL) {
        return;
    }
    memcpy(nested_symbol, "nested", sizeof("nested"));

    FerricValue *nested =
        (FerricValue *)malloc(3 * sizeof(FerricValue));
    CHECK(nested != NULL, "nested C allocation must succeed");
    if (nested == NULL) {
        free(nested_symbol);
        return;
    }
    nested[0] = borrowed_text(FERRIC_VALUE_TYPE_SYMBOL, nested_symbol);
    nested[1] = ferric_value_integer(1234);
    nested[2] = ferric_value_float(2.5);

    FerricValue source[6];
    source[0] = ferric_value_integer(42);
    source[1] = ferric_value_float(3.25);
    source[2] = borrowed_text(FERRIC_VALUE_TYPE_SYMBOL, top_symbol);
    source[3] = borrowed_text(FERRIC_VALUE_TYPE_STRING, top_string);
    source[4] = ferric_value_void();
    source[4].value_type = FERRIC_VALUE_TYPE_MULTIFIELD;
    source[4].multifield_ptr = nested;
    source[4].multifield_len = 3;
    source[5] = ferric_value_void();
    source[5].value_type = FERRIC_VALUE_TYPE_EXTERNAL_ADDRESS;
    source[5].external_type_id = 19;
    source[5].external_pointer = &external_payload;

    FerricValue out = ferric_value_void();
    FerricError err =
        ferric_value_multifield_copy(source, 6, &out);
    CHECK(err == FERRIC_ERROR_OK,
          "mixed stack/malloc tree must copy successfully");
    if (err != FERRIC_ERROR_OK) {
        free(nested);
        free(nested_symbol);
        return;
    }

    CHECK(out.value_type == FERRIC_VALUE_TYPE_MULTIFIELD &&
              out.multifield_len == 6 && out.multifield_ptr != NULL,
          "copy must return a six-element Ferric-owned multifield");
    CHECK(out.multifield_ptr != source,
          "top-level Ferric array must not alias stack input");
    CHECK(out.multifield_ptr[4].multifield_ptr != nested,
          "nested Ferric array must not alias malloc input");
    CHECK(out.multifield_ptr[2].string_ptr != top_symbol &&
              out.multifield_ptr[3].string_ptr != top_string,
          "top-level text storage must be deep-copied");
    CHECK(out.multifield_ptr[4].multifield_ptr[0].string_ptr != nested_symbol,
          "nested text storage must be deep-copied");
    CHECK(out.multifield_ptr[5].external_pointer == &external_payload &&
              out.multifield_ptr[5].external_type_id == 19,
          "external-address payload must remain shallow and caller-owned");

    source[0].integer = -1;
    nested[1].integer = -2;
    top_symbol[0] = 'X';
    top_string[0] = 'Y';
    nested_symbol[0] = 'Z';
    free(nested);
    free(nested_symbol);

    CHECK(out.multifield_ptr[0].integer == 42,
          "top-level scalar copy must survive source mutation");
    CHECK(out.multifield_ptr[4].multifield_ptr[1].integer == 1234,
          "nested scalar copy must survive source destruction");
    CHECK(strcmp(out.multifield_ptr[2].string_ptr, "alpha") == 0,
          "symbol copy must survive source mutation");
    CHECK(strcmp(out.multifield_ptr[3].string_ptr, "hello") == 0,
          "string copy must survive source mutation");
    CHECK(strcmp(out.multifield_ptr[4].multifield_ptr[0].string_ptr,
                 "nested") == 0,
          "nested symbol copy must survive source destruction");

    CHECK(ferric_value_free(&out) == FERRIC_ERROR_OK,
          "Ferric-owned mixed tree must free recursively");
    CHECK(external_payload == 77,
          "freeing the copy must not touch caller-owned external payloads");
}

static void test_partial_failure_cleanup(void) {
    char first[] = "first";
    char second[] = "second";
    char third[] = "third";

    FerricValue nested[3];
    nested[0] = borrowed_text(FERRIC_VALUE_TYPE_SYMBOL, second);
    nested[1] = borrowed_text(FERRIC_VALUE_TYPE_STRING, third);
    nested[2] = ferric_value_void();
    nested[2].value_type = 77u;
    nested[2].string_ptr = (char *)&nested[2];

    FerricValue source[2];
    source[0] = borrowed_text(FERRIC_VALUE_TYPE_STRING, first);
    source[1] = ferric_value_void();
    source[1].value_type = FERRIC_VALUE_TYPE_MULTIFIELD;
    source[1].multifield_ptr = nested;
    source[1].multifield_len = 3;

    for (size_t i = 0; i < 512; i++) {
        FerricValue out = ferric_value_integer((int64_t)i);
        FerricError err =
            ferric_value_multifield_copy(source, 2, &out);
        CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
              "late invalid tag must reject the complete copy");
        check_void(&out, "partial-copy failure must leave output Void");
    }
}

static void test_large_repeated_copy(void) {
    enum {
        ELEMENT_COUNT = 257,
        ROUND_COUNT = 64
    };
    char text[] = "large-borrowed-text";

    for (size_t round = 0; round < ROUND_COUNT; round++) {
        FerricValue *source =
            (FerricValue *)malloc(ELEMENT_COUNT * sizeof(FerricValue));
        CHECK(source != NULL, "large C source allocation must succeed");
        if (source == NULL) {
            return;
        }
        for (size_t i = 0; i < ELEMENT_COUNT; i++) {
            if (i % 3 == 0) {
                source[i] =
                    borrowed_text(FERRIC_VALUE_TYPE_STRING, text);
            } else {
                source[i] = ferric_value_integer((int64_t)(round + i));
            }
        }

        FerricValue out = ferric_value_void();
        FerricError err =
            ferric_value_multifield_copy(source, ELEMENT_COUNT, &out);
        CHECK(err == FERRIC_ERROR_OK,
              "large borrowed array must copy successfully");
        free(source);
        if (err != FERRIC_ERROR_OK) {
            continue;
        }

        text[0] = 'X';
        CHECK(out.multifield_len == ELEMENT_COUNT,
              "large copy must preserve element count");
        CHECK(strcmp(out.multifield_ptr[0].string_ptr,
                     "large-borrowed-text") == 0,
              "large copy must own independent string bytes");
        CHECK(out.multifield_ptr[ELEMENT_COUNT - 1].integer ==
                  (int64_t)(round + ELEMENT_COUNT - 1),
              "large copy must preserve trailing scalar");
        text[0] = 'l';

        CHECK(ferric_value_free(&out) == FERRIC_ERROR_OK,
              "large Ferric-owned tree must free recursively");
    }
}

int main(void) {
    test_empty_and_error_contract();
    test_stack_and_malloc_tree();
    test_partial_failure_cleanup();
    test_large_repeated_copy();

    if (failures == 0) {
        printf("multifield_copy: all checks passed\n");
        return 0;
    }
    fprintf(stderr, "multifield_copy: %d check(s) failed\n", failures);
    return 1;
}
