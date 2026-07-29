/*
 * Native C regression harness for the embedded-NUL boundary policy.
 */

#include "ferric.h"

#include <stdint.h>
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

    if (failures == 0) {
        puts("embedded-NUL harness: checked rejection and legacy behavior passed");
    }
    return failures == 0 ? 0 : 1;
}
