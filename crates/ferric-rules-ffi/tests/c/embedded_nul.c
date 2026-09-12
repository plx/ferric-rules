/*
 * Native C regression harness for the embedded-NUL boundary policy.
 */

#include "ferric.h"

#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>

static int failures = 0;

#define CHECK(condition, message)                                              \
    do {                                                                       \
        if (!(condition)) {                                                    \
            fprintf(stderr, "embedded_nul: %s\n", (message));                  \
            failures++;                                                        \
        }                                                                      \
    } while (0)

typedef enum FerricError (*BytesConstructor)(const uint8_t *, uintptr_t,
                                             struct FerricValue *);

static void check_constructor(BytesConstructor constructor,
                              uint32_t expected_type) {
    const uint8_t embedded[] = {'a', 0, 'b'};
    struct FerricValue value = ferric_value_integer(91);

    CHECK(constructor(embedded, sizeof(embedded), &value) ==
              FERRIC_ERROR_INVALID_ARGUMENT,
          "embedded NUL must return INVALID_ARGUMENT");
    CHECK(value.value_type == FERRIC_VALUE_TYPE_VOID &&
              value.string_ptr == NULL,
          "failed construction must leave a Void output");
    CHECK(ferric_last_error_global() != NULL &&
              strstr(ferric_last_error_global(), "embedded NUL at byte 1") !=
                  NULL,
          "rejection must identify the embedded-NUL byte");

    const uint8_t valid[] = {'h', 0xc3, 0xa9};
    CHECK(constructor(valid, sizeof(valid), &value) == FERRIC_ERROR_OK,
          "valid UTF-8 byte spans must succeed");
    CHECK(value.value_type == expected_type && value.string_ptr != NULL,
          "successful construction must set the requested value type");
    CHECK(value.string_ptr != NULL &&
              memcmp(value.string_ptr, valid, sizeof(valid)) == 0 &&
              value.string_ptr[sizeof(valid)] == '\0',
          "valid UTF-8 bytes must be copied exactly");
    CHECK(ferric_value_free(&value) == FERRIC_ERROR_OK,
          "constructed value must be releasable");

    CHECK(constructor(NULL, 0, &value) == FERRIC_ERROR_OK,
          "NULL with zero length must construct an empty value");
    CHECK(value.string_ptr != NULL && value.string_ptr[0] == '\0',
          "empty value must have a valid terminator");
    CHECK(ferric_value_free(&value) == FERRIC_ERROR_OK,
          "empty value must be releasable");

    CHECK(constructor(NULL, 1, &value) == FERRIC_ERROR_NULL_POINTER,
          "NULL with non-zero length must return NULL_POINTER");
    CHECK(value.value_type == FERRIC_VALUE_TYPE_VOID,
          "null-data failure must leave a Void output");
}

/* Byte tags reuse inactive fields without changing the published 64-bit ABI. */
#if UINTPTR_MAX == UINT64_MAX
FERRIC_STATIC_ASSERT(sizeof(struct FerricValue) == 64, "FerricValue layout changed");
FERRIC_STATIC_ASSERT(offsetof(struct FerricValue, string_ptr) == 24, "string pointer moved");
FERRIC_STATIC_ASSERT(offsetof(struct FerricValue, multifield_len) == 40, "length moved");
#endif

static void check_raw_constructor(BytesConstructor constructor, uint32_t tag) {
    const uint8_t payload[] = {'a', 0, 0xff, 0xc3};
    struct FerricValue value = ferric_value_void();
    CHECK(constructor(payload, sizeof(payload), &value) == FERRIC_ERROR_OK,
          "raw construction must preserve arbitrary bytes");
    CHECK(value.value_type == tag && value.multifield_len == sizeof(payload),
          "raw tag and byte count must be exact");
    CHECK(value.string_ptr != NULL && memcmp(value.string_ptr, payload, sizeof(payload)) == 0,
          "raw payload must match including NUL and invalid UTF-8");
    struct FerricValue copied = ferric_value_void();
    CHECK(ferric_value_multifield_copy(&value, 1, &copied) == FERRIC_ERROR_OK,
          "raw values must support recursive owned copying");
    CHECK(ferric_value_free(&value) == FERRIC_ERROR_OK, "free original raw value");

    struct FerricEngine *engine = ferric_engine_new();
    CHECK(engine != NULL, "create raw output engine");
    CHECK(ferric_engine_load_string(engine,
          "(defrule emit (payload ?x) => (printout t ?x))") == FERRIC_ERROR_OK,
          "load raw output rule");
    CHECK(ferric_engine_reset(engine) == FERRIC_ERROR_OK, "reset raw output engine");
    CHECK(ferric_engine_assert_ordered(engine, "payload", copied.multifield_ptr, 1, NULL) == FERRIC_ERROR_OK,
          "assert copied raw value");
    CHECK(ferric_value_free(&copied) == FERRIC_ERROR_OK, "free recursive raw copy");
    uint64_t fired = 0;
    CHECK(ferric_engine_run(engine, -1, &fired) == FERRIC_ERROR_OK && fired == 1,
          "run raw output rule");
    CHECK(ferric_engine_get_output(engine, "t") == NULL,
          "legacy text output must reject invalid UTF-8");
    uintptr_t needed = 0;
    CHECK(ferric_engine_get_output_copy(engine, "t", NULL, 0, &needed) == FERRIC_ERROR_OK,
          "query exact byte output size");
    const uintptr_t brackets = tag == FERRIC_VALUE_TYPE_INSTANCE_NAME ? 2 : 0;
    CHECK(needed == sizeof(payload) + brackets + 1, "byte output size includes final NUL");
    char output[7] = {0};
    CHECK(ferric_engine_get_output_copy(engine, "t", output, sizeof(output), &needed) == FERRIC_ERROR_OK,
          "copy exact raw output");
    CHECK(memcmp(output + brackets / 2, payload, sizeof(payload)) == 0,
          "raw output bytes must survive the complete native pipeline");
    if (brackets) {
        CHECK(output[0] == '[' && output[5] == ']', "instance-name output keeps brackets");
    }
    CHECK(ferric_engine_free(engine) == FERRIC_ERROR_OK, "free raw output engine");
    CHECK(constructor(NULL, 0, &value) == FERRIC_ERROR_OK, "empty raw value");
    CHECK(value.string_ptr == NULL && value.multifield_len == 0, "empty raw span has no allocation");
    CHECK(ferric_value_free(&value) == FERRIC_ERROR_OK, "free empty raw value");
    CHECK(constructor(NULL, 1, &value) == FERRIC_ERROR_NULL_POINTER, "reject missing raw data");
    CHECK(constructor(payload, UINTPTR_MAX, &value) == FERRIC_ERROR_INVALID_ARGUMENT,
          "reject unaddressable raw span before reading memory");
}

int main(void) {
    char legacy[] = {'a', '\0', 'b', '\0'};
    struct FerricValue value = ferric_value_string(legacy);
    CHECK(value.value_type == FERRIC_VALUE_TYPE_STRING &&
              strcmp(value.string_ptr, "a") == 0,
          "legacy C-string constructor must stop at its first terminator");
    CHECK(ferric_value_free(&value) == FERRIC_ERROR_OK,
          "legacy value must be releasable");

    check_constructor(ferric_value_symbol_bytes, FERRIC_VALUE_TYPE_SYMBOL);
    check_constructor(ferric_value_string_bytes, FERRIC_VALUE_TYPE_STRING);

    check_raw_constructor(ferric_value_string_raw, FERRIC_VALUE_TYPE_STRING_BYTES);
    check_raw_constructor(ferric_value_symbol_raw, FERRIC_VALUE_TYPE_SYMBOL_BYTES);
    check_raw_constructor(ferric_value_instance_name, FERRIC_VALUE_TYPE_INSTANCE_NAME);

    if (failures == 0) {
        puts("embedded-NUL harness: text validation and raw byte preservation passed");
    }
    return failures == 0 ? 0 : 1;
}
