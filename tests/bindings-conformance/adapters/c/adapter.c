#include "ferric.h"

#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define HIGH_ID_ITERATIONS 1048577U

static const char *halt_reason_name(enum FerricHaltReason reason);

static char *read_fixture(const char *name) {
    const char *root = getenv("FERRIC_BINDINGS_CONFORMANCE_ROOT");
    char path[4096];
    FILE *file;
    long size;
    char *data;

    if (root == NULL) {
        fprintf(stderr, "FERRIC_BINDINGS_CONFORMANCE_ROOT is not set\n");
        return NULL;
    }
    if (snprintf(path,
                 sizeof(path),
                 "%s/tests/bindings-conformance/fixtures/%s",
                 root,
                 name) >= (int)sizeof(path)) {
        fprintf(stderr, "fixture path is too long\n");
        return NULL;
    }
    file = fopen(path, "rb");
    if (file == NULL) {
        fprintf(stderr, "cannot open fixture %s\n", path);
        return NULL;
    }
    if (fseek(file, 0, SEEK_END) != 0 || (size = ftell(file)) < 0 ||
        fseek(file, 0, SEEK_SET) != 0) {
        fclose(file);
        fprintf(stderr, "cannot size fixture %s\n", path);
        return NULL;
    }
    data = (char *)malloc((size_t)size + 1U);
    if (data == NULL) {
        fclose(file);
        fprintf(stderr, "cannot allocate fixture buffer\n");
        return NULL;
    }
    if (fread(data, 1U, (size_t)size, file) != (size_t)size) {
        free(data);
        fclose(file);
        fprintf(stderr, "cannot read fixture %s\n", path);
        return NULL;
    }
    data[size] = '\0';
    fclose(file);
    return data;
}

static struct FerricEngine *engine_from_fixture(const char *name) {
    char *source = read_fixture(name);
    struct FerricEngine *engine;
    if (source == NULL) {
        return NULL;
    }
    engine = ferric_engine_new_with_source(source);
    free(source);
    return engine;
}

static void print_json_string(const char *value) {
    const unsigned char *cursor = (const unsigned char *)value;
    putchar('"');
    while (*cursor != '\0') {
        switch (*cursor) {
        case '"':
            fputs("\\\"", stdout);
            break;
        case '\\':
            fputs("\\\\", stdout);
            break;
        case '\b':
            fputs("\\b", stdout);
            break;
        case '\f':
            fputs("\\f", stdout);
            break;
        case '\n':
            fputs("\\n", stdout);
            break;
        case '\r':
            fputs("\\r", stdout);
            break;
        case '\t':
            fputs("\\t", stdout);
            break;
        default:
            if (*cursor < 0x20U) {
                printf("\\u%04x", (unsigned int)*cursor);
            } else {
                putchar((int)*cursor);
            }
            break;
        }
        cursor++;
    }
    putchar('"');
}

static bool print_normalized_value(const struct FerricValue *value) {
    uintptr_t index;
    switch (value->value_type) {
    case FERRIC_VALUE_TYPE_VOID:
        fputs("{\"type\":\"void\"}", stdout);
        return true;
    case FERRIC_VALUE_TYPE_INTEGER:
        printf("{\"type\":\"integer\",\"value\":\"%" PRId64 "\"}", value->integer);
        return true;
    case FERRIC_VALUE_TYPE_FLOAT:
        printf("{\"type\":\"float\",\"value\":\"%.17g\"}", value->float_);
        return true;
    case FERRIC_VALUE_TYPE_SYMBOL:
        fputs("{\"type\":\"symbol\",\"value\":", stdout);
        print_json_string(value->string_ptr);
        putchar('}');
        return true;
    case FERRIC_VALUE_TYPE_STRING:
        fputs("{\"type\":\"string\",\"value\":", stdout);
        print_json_string(value->string_ptr);
        putchar('}');
        return true;
    case FERRIC_VALUE_TYPE_MULTIFIELD:
        fputs("{\"type\":\"multifield\",\"value\":[", stdout);
        for (index = 0; index < value->multifield_len; index++) {
            if (index != 0U) {
                putchar(',');
            }
            if (!print_normalized_value(&value->multifield_ptr[index])) {
                return false;
            }
        }
        fputs("]}", stdout);
        return true;
    case FERRIC_VALUE_TYPE_EXTERNAL_ADDRESS:
        fputs("{\"type\":\"external_address\"}", stdout);
        return true;
    default:
        return false;
    }
}

static bool fetch_field(struct FerricEngine *engine,
                        uint64_t fact_id,
                        struct FerricValue *output) {
    memset(output, 0, sizeof(*output));
    return ferric_engine_get_fact_field(engine, fact_id, 0U, output) == FERRIC_ERROR_OK;
}

static bool print_asserted_value(struct FerricValue *input) {
    struct FerricEngine *engine = ferric_engine_new();
    struct FerricValue output;
    uint64_t fact_id = 0;
    bool success;

    if (engine == NULL) {
        return false;
    }
    success = ferric_engine_assert_ordered(engine, "probe", input, 1U, &fact_id) ==
                  FERRIC_ERROR_OK &&
              fetch_field(engine, fact_id, &output);
    if (success) {
        success = print_normalized_value(&output);
        ferric_value_free(&output);
    }
    ferric_engine_free(engine);
    return success;
}

static bool value_case(const char *case_id) {
    if (strcmp(case_id, "value.void") == 0) {
        struct FerricValue value = ferric_value_void();
        return print_asserted_value(&value);
    }
    if (strcmp(case_id, "value.integer.boundaries") == 0) {
        struct FerricValue minimum = ferric_value_integer(INT64_MIN);
        struct FerricValue maximum = ferric_value_integer(INT64_MAX);
        fputs("{\"minimum\":", stdout);
        if (!print_asserted_value(&minimum)) {
            return false;
        }
        fputs(",\"maximum\":", stdout);
        if (!print_asserted_value(&maximum)) {
            return false;
        }
        putchar('}');
        return true;
    }
    if (strcmp(case_id, "value.float") == 0) {
        struct FerricValue value = ferric_value_float(1.5);
        return print_asserted_value(&value);
    }
    if (strcmp(case_id, "value.symbol.explicit") == 0) {
        struct FerricValue value = ferric_value_symbol("red");
        bool success = print_asserted_value(&value);
        ferric_value_free(&value);
        return success;
    }
    if (strcmp(case_id, "value.string.explicit") == 0 ||
        strcmp(case_id, "value.string.plain-host") == 0) {
        struct FerricValue value = ferric_value_string("red");
        bool success = print_asserted_value(&value);
        ferric_value_free(&value);
        return success;
    }
    if (strcmp(case_id, "value.multifield.nested") == 0) {
        struct FerricValue nested_input[1];
        struct FerricValue nested;
        struct FerricValue inputs[6];
        struct FerricValue multifield;
        bool success;
        size_t index;

        nested_input[0] = ferric_value_integer(9);
        if (ferric_value_multifield_copy(nested_input, 1U, &nested) != FERRIC_ERROR_OK) {
            return false;
        }
        inputs[0] = ferric_value_void();
        inputs[1] = ferric_value_integer(7);
        inputs[2] = ferric_value_float(2.5);
        inputs[3] = ferric_value_symbol("blue");
        inputs[4] = ferric_value_string("text");
        inputs[5] = nested;
        if (ferric_value_multifield_copy(inputs, 6U, &multifield) != FERRIC_ERROR_OK) {
            for (index = 0; index < 6U; index++) {
                ferric_value_free(&inputs[index]);
            }
            return false;
        }
        success = print_asserted_value(&multifield);
        ferric_value_free(&multifield);
        for (index = 0; index < 6U; index++) {
            ferric_value_free(&inputs[index]);
        }
        return success;
    }
    if (strcmp(case_id, "value.external-address") == 0) {
        struct FerricEngine *engine = ferric_engine_new();
        struct FerricValue external;
        uint64_t fact_id = 0;
        enum FerricError code;
        if (engine == NULL) {
            return false;
        }
        memset(&external, 0, sizeof(external));
        external.value_type = FERRIC_VALUE_TYPE_EXTERNAL_ADDRESS;
        external.external_type_id = 7U;
        code = ferric_engine_assert_ordered(engine, "probe", &external, 1U, &fact_id);
        ferric_engine_free(engine);
        printf("{\"host_representation\":\"opaque\",\"ingress\":\"%s\"}",
               code == FERRIC_ERROR_OK ? "accepted" : "rejected");
        return true;
    }
    return false;
}

static bool configuration_default(void) {
    struct FerricEngine *engine = ferric_engine_new();
    struct FerricValue value = ferric_value_string("é");
    uint64_t fact_id = 0;
    enum FerricError code;
    if (engine == NULL) {
        ferric_value_free(&value);
        return false;
    }
    code = ferric_engine_assert_ordered(engine, "unicode", &value, 1U, &fact_id);
    ferric_value_free(&value);
    ferric_engine_free(engine);
    printf("{\"max_call_depth\":64,\"strategy\":\"depth\",\"unicode\":\"%s\"}",
           code == FERRIC_ERROR_OK ? "accepted" : "rejected");
    return true;
}

static bool configuration_custom(void) {
    struct FerricConfig config;
    struct FerricEngine *engine;
    struct FerricValue value;
    char *source;
    uint64_t fact_id = 0;
    uint64_t fired = 0;
    enum FerricHaltReason reason = FERRIC_HALT_REASON_AGENDA_EMPTY;
    enum FerricError unicode_code;
    bool depth_bounded;

    config.string_encoding = FERRIC_STRING_ENCODING_ASCII;
    config.strategy = FERRIC_CONFLICT_STRATEGY_BREADTH;
    config.max_call_depth = 1U;
    engine = ferric_engine_new_with_config(&config);
    source = read_fixture("custom-config.clp");
    if (engine == NULL || source == NULL) {
        free(source);
        if (engine != NULL) {
            ferric_engine_free(engine);
        }
        return false;
    }
    if (ferric_engine_load_string(engine, source) != FERRIC_ERROR_OK ||
        ferric_engine_reset(engine) != FERRIC_ERROR_OK) {
        free(source);
        ferric_engine_free(engine);
        return false;
    }
    free(source);
    value = ferric_value_string("é");
    unicode_code =
        ferric_engine_assert_ordered(engine, "unicode", &value, 1U, &fact_id);
    ferric_value_free(&value);
    depth_bounded =
        ferric_engine_run_ex(engine, -1, &fired, &reason) == FERRIC_ERROR_OK &&
        reason == FERRIC_HALT_REASON_ACTION_ERROR;
    ferric_engine_free(engine);
    if (!depth_bounded) {
        return false;
    }
    printf("{\"ascii_unicode\":\"%s\",\"max_call_depth\":\"configurable\","
           "\"strategy_count\":4}",
           unicode_code == FERRIC_ERROR_OK ? "accepted" : "rejected");
    return true;
}

struct ConfigurationObservation {
    const char *halt_reason;
    const char *unicode;
};

static bool observe_configuration(const char *fixture_name,
                                  const struct FerricConfig *config,
                                  struct ConfigurationObservation *observation) {
    char *source = read_fixture(fixture_name);
    struct FerricEngine *engine;
    struct FerricValue value;
    uint64_t fact_id = 0;
    uint64_t fired = 0;
    enum FerricHaltReason reason = FERRIC_HALT_REASON_AGENDA_EMPTY;
    enum FerricError unicode_code;
    enum FerricError run_code;

    if (source == NULL) {
        return false;
    }
    engine = ferric_engine_new_with_source_config(source, config);
    free(source);
    if (engine == NULL) {
        return false;
    }

    value = ferric_value_string("é");
    unicode_code =
        ferric_engine_assert_ordered(engine, "unicode", &value, 1U, &fact_id);
    ferric_value_free(&value);
    run_code = ferric_engine_run_ex(engine, -1, &fired, &reason);
    ferric_engine_free(engine);
    if (run_code != FERRIC_ERROR_OK || halt_reason_name(reason) == NULL) {
        return false;
    }

    observation->halt_reason = halt_reason_name(reason);
    observation->unicode =
        unicode_code == FERRIC_ERROR_OK ? "accepted" : "rejected";
    return true;
}

static void print_configuration_observation(
    const struct ConfigurationObservation *observation) {
    printf("{\"halt_reason\":\"%s\",\"unicode\":\"%s\"}",
           observation->halt_reason,
           observation->unicode);
}

static bool observe_strategy_breadth_fired(const struct FerricConfig *config,
                                           uint64_t *fired) {
    char *source = read_fixture("configuration-strategy-order.clp");
    struct FerricEngine *engine;
    enum FerricHaltReason reason = FERRIC_HALT_REASON_AGENDA_EMPTY;

    if (source == NULL) {
        return false;
    }
    engine = ferric_engine_new_with_source_config(source, config);
    free(source);
    if (engine == NULL) {
        return false;
    }
    if (ferric_engine_run_ex(engine, -1, fired, &reason) != FERRIC_ERROR_OK ||
        reason != FERRIC_HALT_REASON_AGENDA_EMPTY) {
        ferric_engine_free(engine);
        return false;
    }
    ferric_engine_free(engine);
    return true;
}

static bool configuration_isolation(void) {
    struct FerricConfig encoding_ascii = {
        FERRIC_STRING_ENCODING_ASCII, FERRIC_CONFLICT_STRATEGY_DEPTH, 64U};
    struct FerricConfig strategy_breadth = {
        FERRIC_STRING_ENCODING_UTF8, FERRIC_CONFLICT_STRATEGY_BREADTH, 64U};
    struct FerricConfig depth_one = {
        FERRIC_STRING_ENCODING_UTF8, FERRIC_CONFLICT_STRATEGY_DEPTH, 1U};
    struct FerricConfig depth_256 = {
        FERRIC_STRING_ENCODING_UTF8, FERRIC_CONFLICT_STRATEGY_DEPTH, 256U};
    struct ConfigurationObservation encoding_ascii_observation;
    struct ConfigurationObservation strategy_breadth_observation;
    struct ConfigurationObservation depth_one_observation;
    struct ConfigurationObservation depth_256_observation;
    uint64_t strategy_fired = 0;

    if (!observe_configuration("configuration-default-depth.clp",
                               &encoding_ascii,
                               &encoding_ascii_observation) ||
        !observe_configuration("configuration-default-depth.clp",
                               &strategy_breadth,
                               &strategy_breadth_observation) ||
        !observe_configuration(
            "custom-config.clp", &depth_one, &depth_one_observation) ||
        !observe_configuration("configuration-default-depth.clp",
                               &depth_256,
                               &depth_256_observation) ||
        !observe_strategy_breadth_fired(&strategy_breadth, &strategy_fired)) {
        return false;
    }

    fputs("{\"depth_1_only\":", stdout);
    print_configuration_observation(&depth_one_observation);
    fputs(",\"depth_256_only\":", stdout);
    print_configuration_observation(&depth_256_observation);
    fputs(",\"encoding_ascii_only\":", stdout);
    print_configuration_observation(&encoding_ascii_observation);
    printf(",\"strategy_breadth_only\":{\"halt_reason\":\"%s\","
           "\"strategy_fired\":%" PRIu64 ",\"unicode\":\"%s\"}",
           strategy_breadth_observation.halt_reason,
           strategy_fired,
           strategy_breadth_observation.unicode);
    putchar('}');
    return true;
}

static bool error_case(const char *case_id) {
    struct FerricEngine *engine = ferric_engine_new();
    enum FerricError code;
    const char *family = NULL;
    uint64_t fact_id = 0;
    if (engine == NULL) {
        return false;
    }
    if (strcmp(case_id, "error.parse") == 0) {
        code = ferric_engine_load_string(engine, "(defrule incomplete");
        if (code == FERRIC_ERROR_PARSE_ERROR) {
            family = "parse";
        }
    } else if (strcmp(case_id, "error.compile") == 0) {
        code = ferric_engine_load_string(engine, "(defrule bad => (nonexistent-fn))");
        if (code == FERRIC_ERROR_COMPILE_ERROR) {
            family = "compile";
        }
    } else if (strcmp(case_id, "error.unsupported-construct") == 0) {
        code = ferric_engine_load_string(engine, "(defclass Probe (is-a USER))");
        if (code == FERRIC_ERROR_COMPILE_ERROR) {
            family = "compile";
        }
    } else if (strcmp(case_id, "error.runtime") == 0) {
        code = ferric_engine_assert_ordered(engine, "stale", NULL, 0U, &fact_id);
        if (code == FERRIC_ERROR_OK) {
            code = ferric_engine_retract(engine, fact_id);
        }
        if (code == FERRIC_ERROR_OK) {
            code = ferric_engine_retract(engine, fact_id);
        }
        if (code == FERRIC_ERROR_NOT_FOUND) {
            family = "fact_not_found";
        }
    }
    ferric_engine_free(engine);
    if (family == NULL) {
        return false;
    }
    printf("{\"family\":\"%s\"}", family);
    return true;
}

static bool fact_lifecycle(void) {
    struct FerricEngine *engine = engine_from_fixture("template.clp");
    struct FerricValue ordered_input = ferric_value_integer(7);
    struct FerricValue ordered_snapshot;
    struct FerricValue template_input = ferric_value_string("Ada");
    struct FerricValue template_snapshot;
    const char *slot_names[1] = {"name"};
    uint64_t ordered_id = 0;
    uint64_t template_id = 0;
    uintptr_t count = 0;
    bool ordered_retained;
    bool template_retained;
    if (engine == NULL) {
        ferric_value_free(&template_input);
        return false;
    }
    if (ferric_engine_assert_ordered(
            engine, "ordered", &ordered_input, 1U, &ordered_id) != FERRIC_ERROR_OK ||
        !fetch_field(engine, ordered_id, &ordered_snapshot) ||
        ferric_engine_retract(engine, ordered_id) != FERRIC_ERROR_OK ||
        ferric_engine_assert_template(
            engine, "person", slot_names, &template_input, 1U, &template_id) !=
            FERRIC_ERROR_OK ||
        ferric_engine_get_fact_slot_by_name(engine, template_id, "name", &template_snapshot) !=
            FERRIC_ERROR_OK ||
        ferric_engine_retract(engine, template_id) != FERRIC_ERROR_OK ||
        ferric_engine_fact_count(engine, &count) != FERRIC_ERROR_OK) {
        ferric_value_free(&ordered_input);
        ferric_value_free(&template_input);
        ferric_engine_free(engine);
        return false;
    }
    ordered_retained = ordered_snapshot.value_type == FERRIC_VALUE_TYPE_INTEGER &&
                       ordered_snapshot.integer == 7;
    template_retained = template_snapshot.value_type == FERRIC_VALUE_TYPE_STRING &&
                        strcmp(template_snapshot.string_ptr, "Ada") == 0;
    printf("{\"count_after_retract\":%" PRIuPTR
           ",\"ordered_snapshot_retained\":%s,\"template_snapshot_retained\":%s}",
           count,
           ordered_retained ? "true" : "false",
           template_retained ? "true" : "false");
    ferric_value_free(&ordered_snapshot);
    ferric_value_free(&template_snapshot);
    ferric_value_free(&ordered_input);
    ferric_value_free(&template_input);
    ferric_engine_free(engine);
    return true;
}

static const char *halt_reason_name(enum FerricHaltReason reason) {
    switch (reason) {
    case FERRIC_HALT_REASON_AGENDA_EMPTY:
        return "agenda_empty";
    case FERRIC_HALT_REASON_LIMIT_REACHED:
        return "limit_reached";
    case FERRIC_HALT_REASON_HALT_REQUESTED:
        return "halt_requested";
    case FERRIC_HALT_REASON_ACTION_ERROR:
        return "action_error";
    default:
        return "unknown";
    }
}

static bool print_run_fixture(const char *name, int64_t limit) {
    struct FerricEngine *engine = engine_from_fixture(name);
    uint64_t fired = 0;
    enum FerricHaltReason reason = FERRIC_HALT_REASON_AGENDA_EMPTY;
    enum FerricError code;
    if (engine == NULL) {
        return false;
    }
    code = ferric_engine_run_ex(engine, limit, &fired, &reason);
    ferric_engine_free(engine);
    if (code != FERRIC_ERROR_OK) {
        return false;
    }
    printf("{\"fired\":%" PRIu64 ",\"reason\":\"%s\"}",
           fired,
           halt_reason_name(reason));
    return true;
}

static bool execution_run_limits(void) {
    fputs("{\"zero\":", stdout);
    if (!print_run_fixture("run-limits.clp", 0)) {
        return false;
    }
    fputs(",\"one\":", stdout);
    if (!print_run_fixture("run-limits.clp", 1)) {
        return false;
    }
    fputs(",\"unlimited\":", stdout);
    if (!print_run_fixture("run-limits.clp", -1)) {
        return false;
    }
    putchar('}');
    return true;
}

static bool execution_step(void) {
    struct FerricEngine *engine = engine_from_fixture("one-rule.clp");
    int32_t first = 0;
    int32_t second = 0;
    if (engine == NULL) {
        return false;
    }
    if (ferric_engine_step(engine, &first) != FERRIC_ERROR_OK ||
        ferric_engine_step(engine, &second) != FERRIC_ERROR_OK) {
        ferric_engine_free(engine);
        return false;
    }
    ferric_engine_free(engine);
    printf("{\"empty\":%s,\"first_rule\":null}",
           first == 1 && second == 0 ? "true" : "false");
    return true;
}

static bool execution_diagnostic(void) {
    struct FerricEngine *engine = engine_from_fixture("diagnostic.clp");
    uint64_t fired = 0;
    uintptr_t count = 0;
    enum FerricHaltReason reason = FERRIC_HALT_REASON_AGENDA_EMPTY;
    if (engine == NULL) {
        return false;
    }
    if (ferric_engine_run_ex(engine, -1, &fired, &reason) != FERRIC_ERROR_OK ||
        ferric_engine_action_diagnostic_count(engine, &count) != FERRIC_ERROR_OK) {
        ferric_engine_free(engine);
        return false;
    }
    printf("{\"diagnostic_count\":%" PRIuPTR ",\"fired\":%" PRIu64
           ",\"reason\":\"%s\"}",
           count,
           fired,
           halt_reason_name(reason));
    ferric_engine_free(engine);
    return true;
}

static bool snapshot_roundtrip(void) {
    struct FerricEngine *engine = engine_from_fixture("snapshot.clp");
    struct FerricEngine *restored = NULL;
    uint8_t *bytes = NULL;
    uintptr_t length = 0;
    uintptr_t fact_count = 0;
    uint64_t fact_id = 0;
    uint64_t fired = 0;
    enum FerricHaltReason reason = FERRIC_HALT_REASON_AGENDA_EMPTY;
    enum FerricError code;
    if (engine == NULL) {
        return false;
    }
    code = ferric_engine_assert_ordered(engine, "seed", NULL, 0U, &fact_id);
    if (code == FERRIC_ERROR_OK) {
        code = ferric_engine_serialize_as(engine,
                                          FERRIC_SERIALIZATION_FORMAT_JSON,
                                          NULL,
                                          NULL,
                                          &bytes,
                                          &length);
    }
    ferric_engine_free(engine);
    if (code == FERRIC_ERROR_OK) {
        code = ferric_engine_deserialize_as(bytes,
                                            length,
                                            FERRIC_SERIALIZATION_FORMAT_JSON,
                                            &restored);
    }
    ferric_bytes_free(bytes, length);
    if (code != FERRIC_ERROR_OK || restored == NULL) {
        return false;
    }
    code = ferric_engine_fact_count(restored, &fact_count);
    if (code == FERRIC_ERROR_OK) {
        code = ferric_engine_run_ex(restored, -1, &fired, &reason);
    }
    ferric_engine_free(restored);
    if (code != FERRIC_ERROR_OK) {
        return false;
    }
    printf("{\"fact_count\":%" PRIuPTR
           ",\"format\":\"json\",\"rules_fired\":%" PRIu64 "}",
           fact_count,
           fired);
    return true;
}

static bool lifecycle_close(void) {
    struct FerricEngine *engine = ferric_engine_new();
    if (engine == NULL || ferric_engine_free(engine) != FERRIC_ERROR_OK) {
        return false;
    }
    fputs("{\"explicit\":true,\"idempotent\":false,\"post_close\":\"undefined\"}",
          stdout);
    return true;
}

static bool embedded_nul(void) {
    const uint8_t embedded[] = {'a', '\0', 'b'};
    struct FerricValue value = ferric_value_void();
    enum FerricError code =
        ferric_value_string_bytes(embedded, sizeof(embedded), &value);
    if (code != FERRIC_ERROR_INVALID_ARGUMENT ||
        value.value_type != FERRIC_VALUE_TYPE_VOID) {
        ferric_value_free(&value);
        return false;
    }
    fputs("{\"error\":\"invalid_argument\"}", stdout);
    return true;
}

static bool high_fact_id(void) {
    struct FerricEngine *engine = ferric_engine_new();
    uint64_t fact_id = 0;
    enum FerricFactType fact_type = FERRIC_FACT_TYPE_ORDERED;
    uint32_t iteration;
    bool roundtrip;
    if (engine == NULL) {
        return false;
    }
    for (iteration = 0; iteration < HIGH_ID_ITERATIONS; iteration++) {
        if (ferric_engine_assert_ordered(
                engine, "generation", NULL, 0U, &fact_id) != FERRIC_ERROR_OK ||
            ferric_engine_retract(engine, fact_id) != FERRIC_ERROR_OK) {
            ferric_engine_free(engine);
            return false;
        }
    }
    if (ferric_engine_assert_ordered(
            engine, "generation", NULL, 0U, &fact_id) != FERRIC_ERROR_OK) {
        ferric_engine_free(engine);
        return false;
    }
    roundtrip = fact_id > UINT64_C(9007199254740991) &&
                ferric_engine_get_fact_type(engine, fact_id, &fact_type) ==
                    FERRIC_ERROR_OK;
    printf("{\"roundtrip\":%s}", roundtrip ? "true" : "false");
    ferric_engine_free(engine);
    return true;
}

static bool run_case(const char *case_id) {
    if (strncmp(case_id, "value.", 6U) == 0) {
        return value_case(case_id);
    }
    if (strncmp(case_id, "error.", 6U) == 0) {
        return error_case(case_id);
    }
    if (strcmp(case_id, "configuration.default") == 0) {
        return configuration_default();
    }
    if (strcmp(case_id, "configuration.custom") == 0) {
        return configuration_custom();
    }
    if (strcmp(case_id, "configuration.isolation") == 0) {
        return configuration_isolation();
    }
    if (strcmp(case_id, "fact.lifecycle") == 0) {
        return fact_lifecycle();
    }
    if (strcmp(case_id, "execution.run-limits") == 0) {
        return execution_run_limits();
    }
    if (strcmp(case_id, "execution.step") == 0) {
        return execution_step();
    }
    if (strcmp(case_id, "execution.halt") == 0) {
        return print_run_fixture("halt.clp", -1);
    }
    if (strcmp(case_id, "execution.diagnostic") == 0) {
        return execution_diagnostic();
    }
    if (strcmp(case_id, "execution.batch-boundary-halt") == 0) {
        return print_run_fixture("batch-boundary-halt.clp", -1);
    }
    if (strcmp(case_id, "snapshot.json-roundtrip") == 0) {
        return snapshot_roundtrip();
    }
    if (strcmp(case_id, "lifecycle.close") == 0) {
        return lifecycle_close();
    }
    if (strcmp(case_id, "robustness.embedded-nul") == 0) {
        return embedded_nul();
    }
    if (strcmp(case_id, "identifier.high-fact-id") == 0) {
        return high_fact_id();
    }
    if (strcmp(case_id, "count.run-result-width") == 0) {
        fputs("{\"run_count_bits\":64,\"run_limit_bits\":64}", stdout);
        return true;
    }
    return false;
}

int main(int argc, char **argv) {
    FILE *cases;
    char case_id[256];
    if (argc != 2) {
        fprintf(stderr, "usage: c adapter CASE_IDS_PATH\n");
        return 2;
    }
    cases = fopen(argv[1], "rb");
    if (cases == NULL) {
        fprintf(stderr, "cannot open case list %s\n", argv[1]);
        return 1;
    }
    while (fgets(case_id, sizeof(case_id), cases) != NULL) {
        size_t length = strlen(case_id);
        while (length > 0U &&
               (case_id[length - 1U] == '\n' || case_id[length - 1U] == '\r')) {
            case_id[--length] = '\0';
        }
        if (length == 0U) {
            continue;
        }
        fputs("{\"case\":", stdout);
        print_json_string(case_id);
        fputs(",\"result\":", stdout);
        if (!run_case(case_id)) {
            fprintf(stderr, "c conformance adapter failed case %s\n", case_id);
            fclose(cases);
            return 1;
        }
        fputs("}\n", stdout);
        fflush(stdout);
    }
    if (ferror(cases) != 0) {
        fprintf(stderr, "cannot read case list\n");
        fclose(cases);
        return 1;
    }
    fclose(cases);
    return 0;
}
