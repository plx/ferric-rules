/*
 * discriminant_abuse.c — C-ABI regression harness for FR-CABI-001 (issue #85).
 *
 * Passes invalid `FerricValue.value_type` discriminants — top-level and
 * nested inside multifields — to every API that accepts caller-populated
 * FerricValues, and verifies:
 *
 *   1. Accepting APIs (ferric_engine_assert_ordered,
 *      ferric_engine_assert_template) return FERRIC_ERROR_INVALID_ARGUMENT
 *      for every unknown discriminant, at every nesting depth.
 *   2. Resource-freeing paths (ferric_value_free) never interpret an
 *      unknown discriminant: poison pointers stored alongside an invalid
 *      tag must not be freed (ASan aborts on a wild free).
 *   3. The engine stays usable after each rejection, and valid numeric
 *      discriminants keep their existing meaning.
 *
 * Intended to run under AddressSanitizer + UndefinedBehaviorSanitizer via
 * `just ffi-c-harness` (scripts/ffi-c-harness.sh). The harness exits
 * non-zero on the first contract violation; a sanitizer abort also fails
 * the run.
 *
 * Including ferric.h also compiles its ABI static assertions, which lock
 * discriminant widths and numeric values at compile time.
 */

#include <stdint.h>
#include <stdio.h>
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

/* Discriminants that must be rejected: first invalid value, arbitrary
 * garbage, and bit patterns that would be invalid Rust enum reprs. */
static const uint32_t INVALID_TAGS[] = {7u, 42u, 0xDEADBEEFu, 0xFFFFFFFFu};
static const size_t INVALID_TAG_COUNT =
    sizeof(INVALID_TAGS) / sizeof(INVALID_TAGS[0]);

/* The diagnostic must identify an invalid value_type discriminant and the
 * raw decimal tag. Pre-fix code misclassified out-of-range tags as other
 * value kinds (e.g. 999 as ExternalAddress) and could return
 * INVALID_ARGUMENT with an unrelated message, so message content — not
 * just the error code — is asserted. */
static void check_diag_names_tag(const char *diag, uint32_t tag,
                                 const char *ctx) {
    char needle[64];
    snprintf(needle, sizeof(needle), "invalid value_type discriminant: %u",
             (unsigned)tag);
    CHECK(diag != NULL && strstr(diag, needle) != NULL, ctx);
}

static void expect_engine_usable(FerricEngine *engine) {
    FerricValue ok_field = ferric_value_integer(1);
    uint64_t fact_id = 0;
    FerricError err =
        ferric_engine_assert_ordered(engine, "probe", &ok_field, 1, &fact_id);
    CHECK(err == FERRIC_ERROR_OK, "engine must stay usable after rejection");
    if (err == FERRIC_ERROR_OK) {
        CHECK(ferric_engine_retract(engine, fact_id) == FERRIC_ERROR_OK,
              "probe fact must be retractable");
    }
}

static void test_assert_ordered_top_level(FerricEngine *engine) {
    for (size_t i = 0; i < INVALID_TAG_COUNT; i++) {
        FerricValue bad = ferric_value_void();
        bad.value_type = INVALID_TAGS[i];
        /* Poison the payload fields: they must never be interpreted. */
        bad.string_ptr = (char *)&bad;
        bad.multifield_ptr = (FerricValue *)&bad;
        bad.multifield_len = SIZE_MAX;

        uint64_t fact_id = 0;
        FerricError err =
            ferric_engine_assert_ordered(engine, "bad", &bad, 1, &fact_id);
        CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
              "assert_ordered must reject an invalid top-level discriminant");
        check_diag_names_tag(ferric_engine_last_error(engine), INVALID_TAGS[i],
                             "ordered top-level diagnostic must name the tag");
        expect_engine_usable(engine);
    }
}

static void test_assert_ordered_nested(FerricEngine *engine) {
    for (size_t i = 0; i < INVALID_TAG_COUNT; i++) {
        FerricValue elems[3];
        elems[0] = ferric_value_integer(10);
        elems[1] = ferric_value_void();
        elems[1].value_type = INVALID_TAGS[i];
        elems[1].string_ptr = (char *)&elems[1];
        elems[2] = ferric_value_float(2.5);

        FerricValue mf = ferric_value_void();
        mf.value_type = FERRIC_VALUE_TYPE_MULTIFIELD;
        mf.multifield_ptr = elems;
        mf.multifield_len = 3;

        uint64_t fact_id = 0;
        FerricError err =
            ferric_engine_assert_ordered(engine, "bad", &mf, 1, &fact_id);
        CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
              "assert_ordered must reject an invalid nested discriminant");
        check_diag_names_tag(ferric_engine_last_error(engine), INVALID_TAGS[i],
                             "ordered nested diagnostic must name the tag");
        expect_engine_usable(engine);
    }
}

static void test_assert_ordered_deeply_nested(FerricEngine *engine) {
    FerricValue inner_bad = ferric_value_void();
    inner_bad.value_type = 0xBADBADu;
    inner_bad.multifield_ptr = &inner_bad;
    inner_bad.multifield_len = SIZE_MAX;

    FerricValue inner = ferric_value_void();
    inner.value_type = FERRIC_VALUE_TYPE_MULTIFIELD;
    inner.multifield_ptr = &inner_bad;
    inner.multifield_len = 1;

    FerricValue outer = ferric_value_void();
    outer.value_type = FERRIC_VALUE_TYPE_MULTIFIELD;
    outer.multifield_ptr = &inner;
    outer.multifield_len = 1;

    FerricError err =
        ferric_engine_assert_ordered(engine, "bad", &outer, 1, NULL);
    CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
          "assert_ordered must reject an invalid doubly-nested discriminant");
    check_diag_names_tag(ferric_engine_last_error(engine), 0xBADBADu,
                         "doubly-nested diagnostic must name the tag");
    expect_engine_usable(engine);
}

static void test_assert_template(FerricEngine *engine) {
    const char *slot_names[1] = {"age"};

    for (size_t i = 0; i < INVALID_TAG_COUNT; i++) {
        /* Top-level slot value with an invalid discriminant. */
        FerricValue bad = ferric_value_void();
        bad.value_type = INVALID_TAGS[i];
        bad.string_ptr = (char *)&bad;

        FerricError err = ferric_engine_assert_template(
            engine, "person", slot_names, &bad, 1, NULL);
        CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
              "assert_template must reject an invalid top-level discriminant");
        check_diag_names_tag(ferric_engine_last_error(engine), INVALID_TAGS[i],
                             "template top-level diagnostic must name the tag");
        expect_engine_usable(engine);
    }

    /* Invalid discriminant nested inside a multislot value. */
    const char *tag_slot[1] = {"tags"};
    FerricValue elems[2];
    elems[0] = ferric_value_integer(1);
    elems[1] = ferric_value_void();
    elems[1].value_type = 7u;
    elems[1].string_ptr = (char *)&elems[1];

    FerricValue mf = ferric_value_void();
    mf.value_type = FERRIC_VALUE_TYPE_MULTIFIELD;
    mf.multifield_ptr = elems;
    mf.multifield_len = 2;

    FerricError err = ferric_engine_assert_template(engine, "person", tag_slot,
                                                    &mf, 1, NULL);
    CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
          "assert_template must reject an invalid nested discriminant");
    check_diag_names_tag(ferric_engine_last_error(engine), 7u,
                         "template nested diagnostic must name the tag");
    expect_engine_usable(engine);
}

static void test_value_free_invalid_tag(void) {
    /* An invalid tag must free nothing: the poison pointers below are not
     * heap allocations, so any attempt to interpret/free them aborts under
     * ASan and corrupts memory without it. */
    for (size_t i = 0; i < INVALID_TAG_COUNT; i++) {
        char stack_buf[8] = "poison";
        FerricValue bad = ferric_value_void();
        bad.value_type = INVALID_TAGS[i];
        bad.string_ptr = stack_buf;
        bad.multifield_ptr = (FerricValue *)stack_buf;
        bad.multifield_len = SIZE_MAX;

        FerricError err = ferric_value_free(&bad);
        CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
              "value_free must report an invalid top-level discriminant");
        check_diag_names_tag(ferric_last_error_global(), INVALID_TAGS[i],
                             "value_free diagnostic must name the tag");
        CHECK(stack_buf[0] == 'p', "invalid-tag free must not touch payloads");
    }
}

static void test_value_free_null_and_valid_return_ok(void) {
    CHECK(ferric_value_free(NULL) == FERRIC_ERROR_OK,
          "value_free(NULL) must return OK");
    CHECK(ferric_value_array_free(NULL, 0) == FERRIC_ERROR_OK,
          "value_array_free(NULL, 0) must return OK");
    FerricValue sym = ferric_value_symbol("owned");
    CHECK(ferric_value_free(&sym) == FERRIC_ERROR_OK,
          "value_free of a valid value must return OK");
}

/* Assert a (sym 22) multislot fact and read the slot back, yielding an
 * FFI-allocated multifield array: [Symbol(owned string), Integer].
 * Returns 0 on failure. The caller owns *out and the fact `*fact_id`. */
static int read_back_multislot(FerricEngine *engine, FerricValue *out,
                               uint64_t *fact_id, int64_t discriminator) {
    const char *slot_names[1] = {"tags"};
    FerricValue elems[2];
    elems[0] = ferric_value_symbol("owned-sibling");
    elems[1] = ferric_value_integer(discriminator);
    FerricValue mf = ferric_value_void();
    mf.value_type = FERRIC_VALUE_TYPE_MULTIFIELD;
    mf.multifield_ptr = elems;
    mf.multifield_len = 2;

    FerricError err = ferric_engine_assert_template(engine, "person",
                                                    slot_names, &mf, 1,
                                                    fact_id);
    ferric_value_free(&elems[0]);
    CHECK(err == FERRIC_ERROR_OK, "valid multislot assert must succeed");
    if (err != FERRIC_ERROR_OK) {
        return 0;
    }

    *out = ferric_value_void();
    err = ferric_engine_get_fact_slot_by_name(engine, *fact_id, "tags", out);
    CHECK(err == FERRIC_ERROR_OK, "multislot read-back must succeed");
    if (err != FERRIC_ERROR_OK) {
        return 0;
    }
    CHECK(out->value_type == FERRIC_VALUE_TYPE_MULTIFIELD &&
              out->multifield_len == 2,
          "read-back slot must be a 2-element multifield");
    return out->value_type == FERRIC_VALUE_TYPE_MULTIFIELD &&
           out->multifield_len == 2;
}

static void test_value_free_corrupted_nested_tag(FerricEngine *engine) {
    /* Corrupt a nested tag inside a genuine FFI-allocated multifield, then
     * free via ferric_value_free: it must report the invalid tag, skip the
     * corrupted element's payload, and still free the owned sibling string
     * and the containing array (a leaked sibling fails ASan leak checks). */
    FerricValue out;
    uint64_t fact_id = 0;
    if (!read_back_multislot(engine, &out, &fact_id, 22)) {
        return;
    }

    /* Element 1 is an integer (owns nothing), so corrupting it leaks no
     * memory while its poisoned payload must never be interpreted. */
    out.multifield_ptr[1].value_type = 0xDEADBEEFu;
    out.multifield_ptr[1].string_ptr = (char *)&out;

    FerricError err = ferric_value_free(&out);
    CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
          "value_free must report an invalid nested discriminant");
    check_diag_names_tag(ferric_last_error_global(), 0xDEADBEEFu,
                         "nested value_free diagnostic must name the tag");

    CHECK(ferric_engine_retract(engine, fact_id) == FERRIC_ERROR_OK,
          "fact must be retractable after free");
}

static void test_value_array_free_top_level_invalid(FerricEngine *engine) {
    /* Corrupt a top-level element of an FFI-allocated array, then free the
     * array directly via ferric_value_array_free: it must report the tag
     * while freeing the known sibling string and the array allocation. */
    FerricValue out;
    uint64_t fact_id = 0;
    if (!read_back_multislot(engine, &out, &fact_id, 23)) {
        return;
    }

    out.multifield_ptr[1].value_type = 999u;
    out.multifield_ptr[1].string_ptr = (char *)&out;

    FerricError err =
        ferric_value_array_free(out.multifield_ptr, out.multifield_len);
    CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
          "value_array_free must report an invalid top-level discriminant");
    check_diag_names_tag(ferric_last_error_global(), 999u,
                         "array_free diagnostic must name the tag");

    CHECK(ferric_engine_retract(engine, fact_id) == FERRIC_ERROR_OK,
          "fact must be retractable after array free");
}

static void test_value_array_free_nested_invalid(FerricEngine *engine) {
    /* Nested case: graft one FFI-allocated array as a multifield element of
     * another, corrupt an inner element, and free the outer array. The
     * invalid inner tag must be reported while both arrays and the owned
     * inner sibling string are still freed. */
    FerricValue outer;
    FerricValue inner;
    uint64_t outer_fact = 0;
    uint64_t inner_fact = 0;
    if (!read_back_multislot(engine, &outer, &outer_fact, 24) ||
        !read_back_multislot(engine, &inner, &inner_fact, 25)) {
        return;
    }

    /* Corrupt the inner integer element, then graft the inner array in
     * place of the outer integer element (which owns nothing). */
    inner.multifield_ptr[1].value_type = 0xFFFFFFFFu;
    outer.multifield_ptr[1] = inner;

    FerricError err =
        ferric_value_array_free(outer.multifield_ptr, outer.multifield_len);
    CHECK(err == FERRIC_ERROR_INVALID_ARGUMENT,
          "value_array_free must report an invalid nested discriminant");
    check_diag_names_tag(ferric_last_error_global(), 0xFFFFFFFFu,
                         "nested array_free diagnostic must name the tag");

    CHECK(ferric_engine_retract(engine, outer_fact) == FERRIC_ERROR_OK &&
              ferric_engine_retract(engine, inner_fact) == FERRIC_ERROR_OK,
          "facts must be retractable after nested array free");
}

static void test_valid_values_preserve_meaning(FerricEngine *engine) {
    FerricValue fields[4];
    fields[0] = ferric_value_integer(7);
    fields[1] = ferric_value_float(1.5);
    fields[2] = ferric_value_symbol("sym");
    fields[3] = ferric_value_string("str");

    uint64_t fact_id = 0;
    FerricError err =
        ferric_engine_assert_ordered(engine, "mixed", fields, 4, &fact_id);
    CHECK(err == FERRIC_ERROR_OK, "valid discriminants must still assert");
    CHECK(ferric_value_free(&fields[2]) == FERRIC_ERROR_OK,
          "freeing a valid symbol value must return OK");
    CHECK(ferric_value_free(&fields[3]) == FERRIC_ERROR_OK,
          "freeing a valid string value must return OK");
    if (err != FERRIC_ERROR_OK) {
        return;
    }

    static const uint32_t EXPECTED[4] = {
        FERRIC_VALUE_TYPE_INTEGER,
        FERRIC_VALUE_TYPE_FLOAT,
        FERRIC_VALUE_TYPE_SYMBOL,
        FERRIC_VALUE_TYPE_STRING,
    };
    for (uintptr_t i = 0; i < 4; i++) {
        FerricValue out = ferric_value_void();
        err = ferric_engine_get_fact_field(engine, fact_id, i, &out);
        CHECK(err == FERRIC_ERROR_OK, "field read-back must succeed");
        if (err == FERRIC_ERROR_OK) {
            CHECK(out.value_type == EXPECTED[i],
                  "read-back discriminant must preserve its meaning");
            ferric_value_free(&out);
        }
    }
}

int main(void) {
    FerricEngine *engine = ferric_engine_new();
    CHECK(engine != NULL, "engine creation must succeed");
    if (engine == NULL) {
        return 1;
    }

    const char *tmpl =
        "(deftemplate person (slot name) (slot age) (multislot tags))";
    CHECK(ferric_engine_load_string(engine, tmpl) == FERRIC_ERROR_OK,
          "template load must succeed");
    CHECK(ferric_engine_reset(engine) == FERRIC_ERROR_OK,
          "engine reset must succeed");

    test_assert_ordered_top_level(engine);
    test_assert_ordered_nested(engine);
    test_assert_ordered_deeply_nested(engine);
    test_assert_template(engine);
    test_value_free_invalid_tag();
    test_value_free_null_and_valid_return_ok();
    test_value_free_corrupted_nested_tag(engine);
    test_value_array_free_top_level_invalid(engine);
    test_value_array_free_nested_invalid(engine);
    test_valid_values_preserve_meaning(engine);

    ferric_engine_free(engine);

    if (failures == 0) {
        printf("discriminant_abuse: all checks passed\n");
        return 0;
    }
    fprintf(stderr, "discriminant_abuse: %d check(s) failed\n", failures);
    return 1;
}
