#define _GNU_SOURCE

#include <clips/clips.h>
#include <openssl/evp.h>
#include <openssl/hmac.h>

#include <errno.h>
#include <limits.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define RECORD_PREFIX "__FERRIC_COMPAT_NATIVE_V1__|"
#define MIN_NONCE_LENGTH 32
#define MAX_NONCE_LENGTH 128
#define MAX_TOKEN_LENGTH 128
#define DIGEST_LENGTH 64
#define AUTH_KEY_HEX_LENGTH 64
#define AUTH_KEY_LENGTH 32
#define MAX_CONFIG_LENGTH                                                     \
  (MAX_NONCE_LENGTH + MAX_TOKEN_LENGTH + (2 * DIGEST_LENGTH) +               \
   AUTH_KEY_HEX_LENGTH + 5)
#define MAX_PROBE_PAYLOAD (16U * 1024U * 1024U)

#define NATIVE_EMIT_FUNCTION "ferric-compat-native-emit"
#define NATIVE_COMPLETE_FUNCTION "ferric-compat-native-complete"

struct observer_config {
  char nonce[MAX_NONCE_LENGTH + 1];
  char fixture_id[MAX_TOKEN_LENGTH + 1];
  char source_sha256[DIGEST_LENGTH + 1];
  char composed_sha256[DIGEST_LENGTH + 1];
  unsigned char auth_key[AUTH_KEY_LENGTH];
};

static struct observer_config config;
static void *observer_environment = NULL;
static int observed_halt_rules = 0;
static int observed_halt_execution = 0;
static int observed_evaluation_error = 0;
static int observer_after_run = 0;
static int observer_complete = 0;
static int protocol_violation = 0;

static void write_all(int descriptor, const char *data, size_t length) {
  size_t written = 0;

  while (written < length) {
    const ssize_t result = write(descriptor, data + written, length - written);
    if (result > 0) {
      written += (size_t)result;
    } else if (result < 0 && errno == EINTR) {
      continue;
    } else {
      return;
    }
  }
}

static void fail(const char *message) {
  static const char prefix[] = "ferric CLIPS observer: ";
  write_all(STDERR_FILENO, prefix, sizeof(prefix) - 1);
  write_all(STDERR_FILENO, message, strlen(message));
  write_all(STDERR_FILENO, "\n", 1);
  exit(127);
}

static void emit_authenticated(const char *record, size_t record_length) {
  unsigned char digest[EVP_MAX_MD_SIZE];
  unsigned int digest_length = 0;
  char digest_hex[(2 * AUTH_KEY_LENGTH) + 1];
  static const char hex[] = "0123456789abcdef";

  if (HMAC(EVP_sha256(), config.auth_key, AUTH_KEY_LENGTH,
           (const unsigned char *)record, record_length, digest,
           &digest_length) == NULL ||
      digest_length != AUTH_KEY_LENGTH) {
    fail("could not authenticate a native record");
  }
  for (size_t index = 0; index < AUTH_KEY_LENGTH; index++) {
    digest_hex[2 * index] = hex[digest[index] >> 4];
    digest_hex[(2 * index) + 1] = hex[digest[index] & 0x0f];
  }
  digest_hex[2 * AUTH_KEY_LENGTH] = '\0';

  write_all(STDERR_FILENO, "\n" RECORD_PREFIX, 1 + sizeof(RECORD_PREFIX) - 1);
  write_all(STDERR_FILENO, config.nonce, strlen(config.nonce));
  write_all(STDERR_FILENO, "|", 1);
  write_all(STDERR_FILENO, record, record_length);
  write_all(STDERR_FILENO, "|", 1);
  write_all(STDERR_FILENO, digest_hex, 2 * AUTH_KEY_LENGTH);
  write_all(STDERR_FILENO, "\n", 1);
  (void)memset(digest, 0, sizeof(digest));
  (void)memset(digest_hex, 0, sizeof(digest_hex));
}

static void emit_formatted_record(const char *format, ...) {
  char record[1024];
  va_list arguments;
  int length;

  va_start(arguments, format);
  length = vsnprintf(record, sizeof(record), format, arguments);
  va_end(arguments);
  if (length <= 0 || (size_t)length >= sizeof(record)) {
    fail("native record is too long");
  }
  emit_authenticated(record, (size_t)length);
}

static void emit_issue(const char *issue) {
  emit_formatted_record("ISSUE|%s", issue);
}

static void mark_violation(const char *issue) {
  protocol_violation = 1;
  emit_issue(issue);
}

static void emit_lifecycle(int sequence, const char *event) {
  emit_formatted_record("LIFECYCLE|%d|%s|%s|%s|%s", sequence, event,
                        config.fixture_id, config.source_sha256,
                        config.composed_sha256);
}

static int valid_nonce(const char *nonce) {
  size_t length;

  if (nonce == NULL) {
    return 0;
  }
  length = strlen(nonce);
  if (length < MIN_NONCE_LENGTH || length > MAX_NONCE_LENGTH ||
      (length % 2) != 0) {
    return 0;
  }
  for (size_t index = 0; index < length; index++) {
    const char character = nonce[index];
    if (!((character >= '0' && character <= '9') ||
          (character >= 'a' && character <= 'f'))) {
      return 0;
    }
  }
  return 1;
}

static int valid_token(const char *token) {
  const size_t length = token == NULL ? 0 : strlen(token);

  if (length == 0 || length > MAX_TOKEN_LENGTH) {
    return 0;
  }
  for (size_t index = 0; index < length; index++) {
    const char character = token[index];
    const int alphanumeric =
        (character >= 'A' && character <= 'Z') ||
        (character >= 'a' && character <= 'z') ||
        (character >= '0' && character <= '9');
    const int valid = alphanumeric || character == '.' || character == '_' ||
                      character == ':' || character == '/' || character == '-';
    if (!valid || (index == 0 && !alphanumeric)) {
      return 0;
    }
  }
  return 1;
}

static int valid_digest(const char *digest) {
  if (digest == NULL || strlen(digest) != DIGEST_LENGTH) {
    return 0;
  }
  for (size_t index = 0; index < DIGEST_LENGTH; index++) {
    const char character = digest[index];
    if (!((character >= '0' && character <= '9') ||
          (character >= 'a' && character <= 'f'))) {
      return 0;
    }
  }
  return 1;
}

static int decode_auth_key(const char *encoded,
                           unsigned char output[AUTH_KEY_LENGTH]) {
  if (encoded == NULL || strlen(encoded) != AUTH_KEY_HEX_LENGTH) {
    return 0;
  }
  for (size_t index = 0; index < AUTH_KEY_LENGTH; index++) {
    unsigned int value = 0;
    for (size_t nibble = 0; nibble < 2; nibble++) {
      const char character = encoded[(2 * index) + nibble];
      unsigned int digit;
      if (character >= '0' && character <= '9') {
        digit = (unsigned int)(character - '0');
      } else if (character >= 'a' && character <= 'f') {
        digit = (unsigned int)(character - 'a') + 10U;
      } else {
        return 0;
      }
      value = (value << 4) | digit;
    }
    output[index] = (unsigned char)value;
  }
  return 1;
}

static size_t read_config_line(char *buffer, size_t capacity) {
  size_t used = 0;

  while (used < capacity) {
    char character;
    const ssize_t result = read(STDIN_FILENO, &character, 1);
    if (result == 1) {
      if (character == '\n') {
        return used;
      }
      buffer[used++] = character;
    } else if (result == 0) {
      fprintf(stderr, "ferric CLIPS observer: configuration is truncated\n");
      exit(127);
    } else if (errno != EINTR) {
      fprintf(stderr, "ferric CLIPS observer: cannot read configuration\n");
      exit(127);
    }
  }
  fprintf(stderr, "ferric CLIPS observer: configuration is too long\n");
  exit(127);
}

static void parse_config(char *raw) {
  char *fields[5];
  char *cursor = raw;

  for (size_t index = 0; index < 5; index++) {
    fields[index] = cursor;
    if (index < 4) {
      char *separator = strchr(cursor, '|');
      if (separator == NULL) {
        fprintf(stderr, "ferric CLIPS observer: configuration is truncated\n");
        exit(127);
      }
      *separator = '\0';
      cursor = separator + 1;
    } else if (strchr(cursor, '|') != NULL) {
      fprintf(stderr,
              "ferric CLIPS observer: configuration has extra fields\n");
      exit(127);
    }
  }
  if (!valid_nonce(fields[0]) || !valid_token(fields[1]) ||
      !valid_digest(fields[2]) || !valid_digest(fields[3]) ||
      !decode_auth_key(fields[4], config.auth_key)) {
    fprintf(stderr, "ferric CLIPS observer: configuration is invalid\n");
    exit(127);
  }
  (void)snprintf(config.nonce, sizeof(config.nonce), "%s", fields[0]);
  (void)snprintf(config.fixture_id, sizeof(config.fixture_id), "%s",
                 fields[1]);
  (void)snprintf(config.source_sha256, sizeof(config.source_sha256), "%s",
                 fields[2]);
  (void)snprintf(config.composed_sha256, sizeof(config.composed_sha256), "%s",
                 fields[3]);
}

static void observe_after_firing(void *environment) {
  if (environment != observer_environment) {
    return;
  }
  observed_halt_rules |= EnvGetHaltRules(environment) != 0;
  observed_halt_execution |= GetHaltExecution(environment) != 0;
  observed_evaluation_error |= GetEvaluationError(environment) != 0;
}

static long long count_agenda(void *environment) {
  void *saved_module = EnvGetCurrentModule(environment);
  void *module = NULL;
  long long count = 0;

  while ((module = EnvGetNextDefmodule(environment, module)) != NULL) {
    void *activation = NULL;
    (void)EnvSetCurrentModule(environment, module);
    while ((activation = EnvGetNextActivation(environment, activation)) !=
           NULL) {
      if (count == LLONG_MAX) {
        fprintf(stderr, "ferric CLIPS observer: agenda size overflow\n");
        exit(127);
      }
      count++;
    }
  }
  if (saved_module != NULL) {
    (void)EnvSetCurrentModule(environment, saved_module);
  }
  return count;
}

static int native_emit(void *environment) {
  const char *payload;
  size_t payload_length;
  char header[64];
  int header_length;
  char *record;

  if (environment != observer_environment || !observer_after_run ||
      observer_complete) {
    mark_violation("unauthorized-probe-emission");
    return 0;
  }
  if (EnvRtnArgCount(environment) != 1) {
    mark_violation("probe-argument-count");
    return 0;
  }
  payload = EnvRtnLexeme(environment, 1);
  if (payload == NULL) {
    mark_violation("probe-payload-unavailable");
    return 0;
  }
  payload_length = strlen(payload);
  if (payload_length > MAX_PROBE_PAYLOAD) {
    mark_violation("probe-payload-too-long");
    return 0;
  }
  header_length = snprintf(header, sizeof(header), "PROBE|%zu|", payload_length);
  if (header_length <= 0 || (size_t)header_length >= sizeof(header)) {
    fail("probe header is too long");
  }
  if (payload_length > SIZE_MAX - (size_t)header_length) {
    fail("probe record length overflow");
  }
  record = malloc((size_t)header_length + payload_length);
  if (record == NULL) {
    fail("could not allocate a probe record");
  }
  (void)memcpy(record, header, (size_t)header_length);
  (void)memcpy(record + header_length, payload, payload_length);
  emit_authenticated(record, (size_t)header_length + payload_length);
  (void)memset(record, 0, (size_t)header_length + payload_length);
  free(record);
  return 0;
}

static int native_complete(void *environment) {
  if (environment != observer_environment || !observer_after_run ||
      observer_complete) {
    mark_violation("unauthorized-completion");
    return 0;
  }
  emit_lifecycle(3, "COMPLETE");
  observer_complete = 1;
  return 0;
}

static void evaluate_probe_operations(void *environment) {
  char *line = NULL;
  size_t capacity = 0;
  ssize_t length;

  while ((length = getline(&line, &capacity, stdin)) >= 0) {
    DATA_OBJECT result;
    while (length > 0 &&
           (line[length - 1] == '\n' || line[length - 1] == '\r')) {
      line[--length] = '\0';
    }
    if (length == 0) {
      continue;
    }
    SetEvaluationError(environment, FALSE);
    if (strncmp(line, "(deffunction ", 13) == 0) {
      if (!EnvBuild(environment, line)) {
        mark_violation("probe-evaluation-failed");
      }
    } else {
      (void)EnvEval(environment, line, &result);
    }
    if (GetEvaluationError(environment)) {
      mark_violation("probe-evaluation-failed");
    }
  }
  free(line);
  if (ferror(stdin)) {
    fprintf(stderr, "ferric CLIPS observer: cannot read probe operations\n");
    exit(127);
  }
}

int main(int argc, char **argv) {
  char raw_config[MAX_CONFIG_LENGTH + 1];
  const char *source_path;
  void *environment;
  long long rules_fired;
  long long agenda_size;
  int result = 0;

  if (argc != 3 || strcmp(argv[1], "--source") != 0) {
    fprintf(stderr,
            "usage: ferric-clips-observer --source <repository-path>\n");
    return 127;
  }
  source_path = argv[2];
  const size_t config_length =
      read_config_line(raw_config, sizeof(raw_config) - 1);
  raw_config[config_length] = '\0';
  parse_config(raw_config);
  (void)memset(raw_config, 0, sizeof(raw_config));

  environment = CreateEnvironment();
  if (environment == NULL) {
    fprintf(stderr, "ferric CLIPS observer: cannot create environment\n");
    return 127;
  }
  observer_environment = environment;
  if (!EnvDefineFunction2(environment, NATIVE_EMIT_FUNCTION, 'v', native_emit,
                          "ferric_compat_native_emit", "11s") ||
      !EnvDefineFunction2(environment, NATIVE_COMPLETE_FUNCTION, 'v',
                          native_complete, "ferric_compat_native_complete",
                          "00")) {
    fprintf(stderr,
            "ferric CLIPS observer: cannot register observer functions\n");
    (void)DestroyEnvironment(environment);
    return 127;
  }

  if (EnvLoad(environment, source_path) != 1) {
    mark_violation("source-load-failed");
    (void)DestroyEnvironment(environment);
    return 1;
  }

  emit_lifecycle(0, "START");
  EnvReset(environment);
  observed_halt_rules = 0;
  observed_halt_execution = 0;
  observed_evaluation_error = 0;
  if (!EnvAddRunFunction(environment, "ferric-native-observer",
                         observe_after_firing, 3000)) {
    fprintf(stderr, "ferric CLIPS observer: cannot register run callback\n");
    (void)DestroyEnvironment(environment);
    return 127;
  }
  rules_fired = EnvRun(environment, -1);
  observe_after_firing(environment);
  if (!EnvRemoveRunFunction(environment, "ferric-native-observer")) {
    fprintf(stderr, "ferric CLIPS observer: cannot remove run callback\n");
    (void)DestroyEnvironment(environment);
    return 127;
  }
  agenda_size = count_agenda(environment);
  emit_formatted_record(
      "RUN|-1|%lld|%d|%d|%d|%lld|%d", rules_fired,
      observed_halt_rules != 0,
      observed_halt_execution != 0, observed_evaluation_error != 0,
      agenda_size, protocol_violation != 0);
  observer_after_run = 1;

  evaluate_probe_operations(environment);
  if (!observer_complete) {
    mark_violation("completion-missing");
  }
  if (protocol_violation) {
    result = 1;
  }
  if (!DestroyEnvironment(environment)) {
    fprintf(stderr, "ferric CLIPS observer: cannot destroy environment\n");
    return 127;
  }
  observer_environment = NULL;
  (void)memset(&config, 0, sizeof(config));
  return result;
}
