#define _GNU_SOURCE

#include <clips/clips.h>
#include <openssl/evp.h>
#include <openssl/hmac.h>

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define RECORD_PREFIX "__FERRIC_COMPAT_NATIVE_V1__|"
#define MIN_NONCE_LENGTH 32
#define MAX_NONCE_LENGTH 128
#define MAX_TOKEN_LENGTH 128
#define DIGEST_LENGTH 64
#define AUTH_KEY_HEX_LENGTH 64
#define AUTH_KEY_LENGTH 32
#define DIAGNOSTIC_TAXONOMY_VERSION 1
#define MAX_CONFIG_LENGTH                                                     \
  (MAX_NONCE_LENGTH + MAX_TOKEN_LENGTH + (2 * DIGEST_LENGTH) +               \
   AUTH_KEY_HEX_LENGTH + 5)
#define MAX_PROBE_PAYLOAD (16U * 1024U * 1024U)
#define MAX_DIAGNOSTIC_PAYLOAD MAX_PROBE_PAYLOAD
#define MAX_SCENARIO_BYTES (1024U * 1024U)
#define MAX_SCENARIO_LINE 4095U
#define MAX_SCENARIO_PATH 4095U
#define MAX_SCENARIO_SOURCES 64U
#define MAX_SCENARIO_STEPS 256U
#define MAX_SCENARIO_SOURCE_BYTES (16U * 1024U * 1024U)
#define MAX_SCENARIO_TOTAL_SOURCE_BYTES (64U * 1024U * 1024U)

#define REPOSITORY_ROOT "/workspace"
#define SCENARIO_HEADER "FERRIC-COMPAT-SCENARIO|1"

#define NATIVE_EMIT_FUNCTION "ferric-compat-native-emit"
#define NATIVE_COMPLETE_FUNCTION "ferric-compat-native-complete"
#define DIAGNOSTIC_ROUTER_NAME "ferric-diagnostic-observer"

struct observer_config {
  char nonce[MAX_NONCE_LENGTH + 1];
  char fixture_id[MAX_TOKEN_LENGTH + 1];
  char source_sha256[DIGEST_LENGTH + 1];
  char composed_sha256[DIGEST_LENGTH + 1];
  unsigned char auth_key[AUTH_KEY_LENGTH];
};

struct diagnostic_buffer {
  char *data;
  size_t length;
  size_t capacity;
};

enum scenario_step_kind {
  SCENARIO_LOAD,
  SCENARIO_RESET,
  SCENARIO_SET_STRATEGY,
  SCENARIO_RUN
};

enum scenario_failure_policy {
  SCENARIO_STOP,
  SCENARIO_CONTINUE
};

struct scenario_source {
  char name[MAX_TOKEN_LENGTH + 1];
  char sha256[DIGEST_LENGTH + 1];
  char path[MAX_SCENARIO_PATH + 1];
  char resolved_path[MAX_SCENARIO_PATH + 1];
};

struct scenario_step {
  enum scenario_step_kind kind;
  enum scenario_failure_policy policy;
  size_t source_index;
  int strategy;
};

struct scenario_plan {
  struct scenario_source *sources;
  size_t source_count;
  size_t total_source_bytes;
  struct scenario_step *steps;
  size_t step_count;
};

static struct observer_config config;
static void *observer_environment = NULL;
static int observed_halt_rules = 0;
static int observed_halt_execution = 0;
static int observed_evaluation_error = 0;
static int observer_after_run = 0;
static int observer_complete = 0;
static int protocol_violation = 0;
static struct diagnostic_buffer diagnostic_output = {NULL, 0, 0};
static const char *active_diagnostic_phase = NULL;
static int phase_sequence = 0;

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

static void emit_phase(const char *phase, const char *event,
                       const char *status) {
  phase_sequence++;
  if (status == NULL) {
    emit_formatted_record("PHASE|%d|%s|%s", phase_sequence, phase, event);
  } else {
    emit_formatted_record("PHASE|%d|%s|%s|%s", phase_sequence, phase, event,
                          status);
  }
}

static void emit_diagnostic(const char *phase, int continued,
                            const char *payload, size_t payload_length) {
  char header[96];
  int header_length;
  char *record;

  header_length = snprintf(header, sizeof(header), "DIAGNOSTIC|%d|%s|%d|%zu|",
                           DIAGNOSTIC_TAXONOMY_VERSION, phase,
                           continued != 0, payload_length);
  if (header_length <= 0 || (size_t)header_length >= sizeof(header)) {
    fail("diagnostic header is too long");
  }
  if (payload_length > MAX_DIAGNOSTIC_PAYLOAD ||
      payload_length > SIZE_MAX - (size_t)header_length) {
    fail("diagnostic record length overflow");
  }
  record = malloc((size_t)header_length + payload_length);
  if (record == NULL) {
    fail("could not allocate a diagnostic record");
  }
  (void)memcpy(record, header, (size_t)header_length);
  if (payload_length != 0) {
    (void)memcpy(record + header_length, payload, payload_length);
  }
  emit_authenticated(record, (size_t)header_length + payload_length);
  (void)memset(record, 0, (size_t)header_length + payload_length);
  free(record);
}

static void append_diagnostic_output(const char *text) {
  const size_t text_length = strlen(text);
  size_t required;
  size_t capacity;
  char *resized;

  if (text_length > MAX_DIAGNOSTIC_PAYLOAD - diagnostic_output.length) {
    fail("CLIPS diagnostic output is too long");
  }
  required = diagnostic_output.length + text_length;
  if (required <= diagnostic_output.capacity) {
    (void)memcpy(diagnostic_output.data + diagnostic_output.length, text,
                 text_length);
    diagnostic_output.length = required;
    return;
  }

  capacity = diagnostic_output.capacity == 0 ? 1024 : diagnostic_output.capacity;
  while (capacity < required) {
    if (capacity > MAX_DIAGNOSTIC_PAYLOAD / 2) {
      capacity = MAX_DIAGNOSTIC_PAYLOAD;
    } else {
      capacity *= 2;
    }
  }
  resized = realloc(diagnostic_output.data, capacity);
  if (resized == NULL) {
    fail("could not allocate CLIPS diagnostic output");
  }
  diagnostic_output.data = resized;
  diagnostic_output.capacity = capacity;
  (void)memcpy(diagnostic_output.data + diagnostic_output.length, text,
               text_length);
  diagnostic_output.length = required;
}

static int diagnostic_router_query(void *environment,
                                   const char *logical_name) {
  if (environment != observer_environment || active_diagnostic_phase == NULL) {
    return 0;
  }
  return strcmp(logical_name, "werror") == 0 ||
         strcmp(logical_name, "wwarning") == 0;
}

static int diagnostic_router_print(void *environment,
                                   const char *logical_name,
                                   const char *text) {
  (void)logical_name;
  if (environment != observer_environment || active_diagnostic_phase == NULL ||
      text == NULL) {
    return 0;
  }
  write_all(STDERR_FILENO, text, strlen(text));
  append_diagnostic_output(text);
  return 1;
}

static void begin_diagnostic_phase(const char *phase) {
  if (active_diagnostic_phase != NULL) {
    fail("diagnostic phases overlap");
  }
  diagnostic_output.length = 0;
  emit_phase(phase, "BEGIN", NULL);
  active_diagnostic_phase = phase;
}

static void end_diagnostic_phase(const char *phase, int has_diagnostic,
                                 int continued) {
  const char *status;

  if (active_diagnostic_phase == NULL ||
      strcmp(active_diagnostic_phase, phase) != 0) {
    fail("diagnostic phase ended out of order");
  }
  active_diagnostic_phase = NULL;
  if (has_diagnostic) {
    emit_diagnostic(phase, continued, diagnostic_output.data,
                    diagnostic_output.length);
    status = continued ? "CONTINUED" : "ERROR";
  } else {
    status = "OK";
  }
  diagnostic_output.length = 0;
  emit_phase(phase, "END", status);
}

static void release_diagnostic_output(void) {
  if (diagnostic_output.data != NULL) {
    (void)memset(diagnostic_output.data, 0, diagnostic_output.capacity);
    free(diagnostic_output.data);
  }
  diagnostic_output.data = NULL;
  diagnostic_output.length = 0;
  diagnostic_output.capacity = 0;
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

static int valid_utf8(const unsigned char *data, size_t length) {
  size_t index = 0;

  while (index < length) {
    const unsigned char first = data[index++];
    if (first <= 0x7fU) {
      continue;
    }
    if (first >= 0xc2U && first <= 0xdfU) {
      if (index >= length || data[index] < 0x80U || data[index] > 0xbfU) {
        return 0;
      }
      index++;
      continue;
    }
    if (first >= 0xe0U && first <= 0xefU) {
      unsigned char second;
      if (index + 1 >= length) {
        return 0;
      }
      second = data[index];
      if (second < 0x80U || second > 0xbfU || data[index + 1] < 0x80U ||
          data[index + 1] > 0xbfU || (first == 0xe0U && second < 0xa0U) ||
          (first == 0xedU && second > 0x9fU)) {
        return 0;
      }
      index += 2;
      continue;
    }
    if (first >= 0xf0U && first <= 0xf4U) {
      unsigned char second;
      if (index + 2 >= length) {
        return 0;
      }
      second = data[index];
      if (second < 0x80U || second > 0xbfU || data[index + 1] < 0x80U ||
          data[index + 1] > 0xbfU || data[index + 2] < 0x80U ||
          data[index + 2] > 0xbfU || (first == 0xf0U && second < 0x90U) ||
          (first == 0xf4U && second > 0x8fU)) {
        return 0;
      }
      index += 3;
      continue;
    }
    return 0;
  }
  return 1;
}

static void digest_to_hex(const unsigned char *digest, size_t digest_length,
                          char output[DIGEST_LENGTH + 1]) {
  static const char hex[] = "0123456789abcdef";

  if (digest_length != DIGEST_LENGTH / 2) {
    fail("SHA-256 digest length is invalid");
  }
  for (size_t index = 0; index < digest_length; index++) {
    output[2 * index] = hex[digest[index] >> 4];
    output[(2 * index) + 1] = hex[digest[index] & 0x0fU];
  }
  output[DIGEST_LENGTH] = '\0';
}

static void sha256_bytes(const unsigned char *data, size_t length,
                         char output[DIGEST_LENGTH + 1]) {
  unsigned char digest[EVP_MAX_MD_SIZE];
  unsigned int digest_length = 0;

  if (!EVP_Digest(data, length, digest, &digest_length, EVP_sha256(), NULL)) {
    fail("could not hash scenario bytes");
  }
  digest_to_hex(digest, digest_length, output);
  (void)memset(digest, 0, sizeof(digest));
}

static void sha256_file(const char *path,
                        char output[DIGEST_LENGTH + 1]) {
  unsigned char buffer[16384];
  unsigned char digest[EVP_MAX_MD_SIZE];
  unsigned int digest_length = 0;
  EVP_MD_CTX *context;
  FILE *file;
  size_t length;

  file = fopen(path, "rb");
  if (file == NULL) {
    fail("could not open a scenario source");
  }
  context = EVP_MD_CTX_new();
  if (context == NULL ||
      !EVP_DigestInit_ex(context, EVP_sha256(), NULL)) {
    if (context != NULL) {
      EVP_MD_CTX_free(context);
    }
    (void)fclose(file);
    fail("could not initialize source hashing");
  }
  while ((length = fread(buffer, 1, sizeof(buffer), file)) != 0) {
    if (!EVP_DigestUpdate(context, buffer, length)) {
      EVP_MD_CTX_free(context);
      (void)fclose(file);
      fail("could not hash a scenario source");
    }
  }
  if (ferror(file) ||
      !EVP_DigestFinal_ex(context, digest, &digest_length)) {
    EVP_MD_CTX_free(context);
    (void)fclose(file);
    fail("could not finish source hashing");
  }
  EVP_MD_CTX_free(context);
  if (fclose(file) != 0) {
    fail("could not close a scenario source");
  }
  digest_to_hex(digest, digest_length, output);
  (void)memset(buffer, 0, sizeof(buffer));
  (void)memset(digest, 0, sizeof(digest));
}

static int contained_in_repository(const char *path) {
  const size_t root_length = sizeof(REPOSITORY_ROOT) - 1;

  return strncmp(path, REPOSITORY_ROOT, root_length) == 0 &&
         path[root_length] == '/';
}

static void resolve_regular_repository_file(
    const char *path, char output[MAX_SCENARIO_PATH + 1]) {
  char resolved[MAX_SCENARIO_PATH + 1];
  struct stat link_status;
  struct stat status;

  if (lstat(path, &link_status) != 0 || S_ISLNK(link_status.st_mode) ||
      realpath(path, resolved) == NULL || !contained_in_repository(resolved) ||
      stat(resolved, &status) != 0 || !S_ISREG(status.st_mode)) {
    fail("scenario path is not a contained regular file");
  }
  if (strlen(resolved) > MAX_SCENARIO_PATH) {
    fail("resolved scenario path is too long");
  }
  (void)snprintf(output, MAX_SCENARIO_PATH + 1, "%s", resolved);
}

static int valid_source_path(const char *path) {
  static const char prefix[] = "tests/examples/";
  const size_t length = path == NULL ? 0 : strlen(path);
  const char *segment;

  if (length <= sizeof(prefix) - 1 || length > MAX_SCENARIO_PATH ||
      strncmp(path, prefix, sizeof(prefix) - 1) != 0 || path[0] == '/' ||
      path[length - 1] == '/') {
    return 0;
  }
  segment = path;
  for (size_t index = 0; index <= length; index++) {
    const unsigned char character = (unsigned char)path[index];
    if (character == '\\' || character == '|' ||
        (character != '\0' && (character < 0x20U || character == 0x7fU))) {
      return 0;
    }
    if (character == '/' || character == '\0') {
      const size_t segment_length = (size_t)(&path[index] - segment);
      if (segment_length == 0 ||
          (segment_length == 1 && segment[0] == '.') ||
          (segment_length == 2 && segment[0] == '.' && segment[1] == '.')) {
        return 0;
      }
      segment = &path[index + 1];
    }
  }
  return valid_utf8((const unsigned char *)path, length);
}

static void resolve_scenario_source(
    const char *relative_path, char output[MAX_SCENARIO_PATH + 1]) {
  char candidate[MAX_SCENARIO_PATH + 1];
  const int length = snprintf(candidate, sizeof(candidate), "%s/%s",
                              REPOSITORY_ROOT, relative_path);

  if (length <= 0 || (size_t)length >= sizeof(candidate)) {
    fail("scenario source path is too long");
  }
  resolve_regular_repository_file(candidate, output);
}

static size_t bounded_scenario_source_size(const char *path) {
  struct stat status;

  if (stat(path, &status) != 0 || !S_ISREG(status.st_mode) ||
      status.st_size < 0 ||
      (uintmax_t)status.st_size > MAX_SCENARIO_SOURCE_BYTES) {
    fail("scenario source size is invalid");
  }
  return (size_t)status.st_size;
}

static char *read_scenario_file(const char *path, size_t *output_length) {
  char resolved[MAX_SCENARIO_PATH + 1];
  struct stat status;
  char *data;
  size_t used = 0;
  int descriptor;

  resolve_regular_repository_file(path, resolved);
  descriptor = open(resolved, O_RDONLY | O_CLOEXEC);
  if (descriptor < 0 || fstat(descriptor, &status) != 0 ||
      !S_ISREG(status.st_mode) || status.st_size <= 0 ||
      (uintmax_t)status.st_size > MAX_SCENARIO_BYTES) {
    if (descriptor >= 0) {
      (void)close(descriptor);
    }
    fail("scenario plan size is invalid");
  }
  data = malloc((size_t)status.st_size + 1);
  if (data == NULL) {
    (void)close(descriptor);
    fail("could not allocate scenario plan bytes");
  }
  while (used < (size_t)status.st_size) {
    const ssize_t length =
        read(descriptor, data + used, (size_t)status.st_size - used);
    if (length > 0) {
      used += (size_t)length;
    } else if (length < 0 && errno == EINTR) {
      continue;
    } else {
      (void)close(descriptor);
      free(data);
      fail("could not read complete scenario plan bytes");
    }
  }
  if (close(descriptor) != 0) {
    free(data);
    fail("could not close scenario plan");
  }
  data[used] = '\0';
  *output_length = used;
  return data;
}

static size_t split_fields(char *line, char **fields, size_t capacity) {
  size_t count = 1;

  fields[0] = line;
  for (char *cursor = line; *cursor != '\0'; cursor++) {
    if (*cursor == '|') {
      *cursor = '\0';
      if (count == capacity) {
        return capacity + 1;
      }
      fields[count++] = cursor + 1;
    }
  }
  return count;
}

static enum scenario_failure_policy parse_failure_policy(const char *text) {
  if (strcmp(text, "stop") == 0) {
    return SCENARIO_STOP;
  }
  if (strcmp(text, "continue") == 0) {
    return SCENARIO_CONTINUE;
  }
  fail("scenario step failure policy is invalid");
  return SCENARIO_STOP;
}

static size_t scenario_source_index(const struct scenario_plan *plan,
                                    const char *name) {
  for (size_t index = 0; index < plan->source_count; index++) {
    if (strcmp(plan->sources[index].name, name) == 0) {
      return index;
    }
  }
  return SIZE_MAX;
}

static int scenario_strategy(const char *name) {
  if (strcmp(name, "depth") == 0) {
    return DEPTH_STRATEGY;
  }
  if (strcmp(name, "breadth") == 0) {
    return BREADTH_STRATEGY;
  }
  if (strcmp(name, "lex") == 0) {
    return LEX_STRATEGY;
  }
  if (strcmp(name, "mea") == 0) {
    return MEA_STRATEGY;
  }
  fail("scenario strategy is invalid");
  return DEPTH_STRATEGY;
}

static void parse_scenario_source(struct scenario_plan *plan, char *line) {
  char *fields[4];
  char actual_digest[DIGEST_LENGTH + 1];
  struct scenario_source *source;
  size_t source_size;

  if (split_fields(line, fields, 4) != 4 ||
      strcmp(fields[0], "SOURCE") != 0 || !valid_token(fields[1]) ||
      !valid_digest(fields[2]) || !valid_source_path(fields[3])) {
    fail("scenario SOURCE record is invalid");
  }
  if (plan->source_count == MAX_SCENARIO_SOURCES ||
      scenario_source_index(plan, fields[1]) != SIZE_MAX) {
    fail("scenario SOURCE inventory is invalid");
  }
  for (size_t index = 0; index < plan->source_count; index++) {
    if (strcmp(plan->sources[index].path, fields[3]) == 0) {
      fail("scenario SOURCE paths must be unique");
    }
  }
  if (plan->source_count == 0 &&
      (strcmp(fields[1], "primary") != 0 ||
       strcmp(fields[2], config.source_sha256) != 0)) {
    fail("scenario primary source identity is invalid");
  }

  source = &plan->sources[plan->source_count];
  (void)snprintf(source->name, sizeof(source->name), "%s", fields[1]);
  (void)snprintf(source->sha256, sizeof(source->sha256), "%s", fields[2]);
  (void)snprintf(source->path, sizeof(source->path), "%s", fields[3]);
  resolve_scenario_source(source->path, source->resolved_path);
  source_size = bounded_scenario_source_size(source->resolved_path);
  if (source_size > MAX_SCENARIO_TOTAL_SOURCE_BYTES -
                        plan->total_source_bytes) {
    fail("scenario aggregate source size is invalid");
  }
  plan->total_source_bytes += source_size;
  sha256_file(source->resolved_path, actual_digest);
  if (strcmp(actual_digest, source->sha256) != 0) {
    fail("scenario source digest does not match");
  }
  (void)memset(actual_digest, 0, sizeof(actual_digest));
  plan->source_count++;
}

static void parse_scenario_step(struct scenario_plan *plan, char *line,
                                int *saw_run) {
  char *fields[5];
  char expected_sequence[32];
  struct scenario_step *step;
  enum scenario_failure_policy policy;
  size_t source_index;

  if (split_fields(line, fields, 5) != 5 ||
      strcmp(fields[0], "STEP") != 0 || *saw_run ||
      plan->step_count == MAX_SCENARIO_STEPS) {
    fail("scenario STEP record is invalid");
  }
  (void)snprintf(expected_sequence, sizeof(expected_sequence), "%zu",
                 plan->step_count + 1);
  if (strcmp(fields[1], expected_sequence) != 0) {
    fail("scenario STEP sequence is invalid");
  }
  policy = parse_failure_policy(fields[4]);
  step = &plan->steps[plan->step_count];
  step->policy = policy;

  if (strcmp(fields[2], "LOAD") == 0) {
    source_index = scenario_source_index(plan, fields[3]);
    if (source_index == SIZE_MAX) {
      fail("scenario LOAD source is not declared");
    }
    step->kind = SCENARIO_LOAD;
    step->source_index = source_index;
  } else if (strcmp(fields[2], "RESET") == 0) {
    if (strcmp(fields[3], "-") != 0) {
      fail("scenario RESET argument is invalid");
    }
    step->kind = SCENARIO_RESET;
  } else if (strcmp(fields[2], "SET-STRATEGY") == 0) {
    if (policy != SCENARIO_STOP) {
      fail("scenario SET-STRATEGY policy must be stop");
    }
    step->kind = SCENARIO_SET_STRATEGY;
    step->strategy = scenario_strategy(fields[3]);
  } else if (strcmp(fields[2], "RUN") == 0) {
    if (strcmp(fields[3], "-1") != 0 || policy != SCENARIO_STOP) {
      fail("scenario RUN record is invalid");
    }
    step->kind = SCENARIO_RUN;
    *saw_run = 1;
  } else {
    fail("scenario STEP operation is invalid");
  }
  plan->step_count++;
}

static struct scenario_plan parse_scenario_plan(const char *path) {
  struct scenario_plan plan = {NULL, 0, 0, NULL, 0};
  char actual_digest[DIGEST_LENGTH + 1];
  char *data;
  char *cursor;
  char *end;
  size_t length;
  size_t line_number = 0;
  int saw_steps = 0;
  int saw_run = 0;
  int saw_end = 0;
  int saw_load = 0;
  int saw_primary_load = 0;
  int saw_reset = 0;
  int saw_strategy = 0;

  data = read_scenario_file(path, &length);
  if (data[length - 1] != '\n' || memchr(data, '\r', length) != NULL ||
      memchr(data, '\0', length) != NULL ||
      !valid_utf8((const unsigned char *)data, length)) {
    free(data);
    fail("scenario plan encoding is invalid");
  }
  sha256_bytes((const unsigned char *)data, length, actual_digest);
  if (strcmp(actual_digest, config.composed_sha256) != 0) {
    free(data);
    fail("scenario plan digest does not match");
  }
  (void)memset(actual_digest, 0, sizeof(actual_digest));

  plan.sources = calloc(MAX_SCENARIO_SOURCES, sizeof(*plan.sources));
  plan.steps = calloc(MAX_SCENARIO_STEPS, sizeof(*plan.steps));
  if (plan.sources == NULL || plan.steps == NULL) {
    free(plan.sources);
    free(plan.steps);
    free(data);
    fail("could not allocate scenario plan");
  }

  cursor = data;
  end = data + length;
  while (cursor < end) {
    char *newline = memchr(cursor, '\n', (size_t)(end - cursor));
    size_t line_length;

    if (newline == NULL) {
      free(data);
      fail("scenario plan is missing its final newline");
    }
    line_length = (size_t)(newline - cursor);
    if (line_length == 0 || line_length > MAX_SCENARIO_LINE) {
      free(data);
      fail("scenario plan line length is invalid");
    }
    *newline = '\0';
    line_number++;

    if (line_number == 1) {
      if (strcmp(cursor, SCENARIO_HEADER) != 0) {
        free(data);
        fail("scenario header is invalid");
      }
    } else if (strcmp(cursor, "END") == 0) {
      if (newline + 1 != end) {
        free(data);
        fail("scenario END record is not terminal");
      }
      saw_end = 1;
      break;
    } else if (strncmp(cursor, "SOURCE|", 7) == 0) {
      if (saw_steps) {
        free(data);
        fail("scenario SOURCE follows a STEP");
      }
      parse_scenario_source(&plan, cursor);
    } else if (strncmp(cursor, "STEP|", 5) == 0) {
      saw_steps = 1;
      parse_scenario_step(&plan, cursor, &saw_run);
    } else {
      free(data);
      fail("scenario record kind is invalid");
    }
    cursor = newline + 1;
  }
  free(data);
  if (!saw_end || plan.source_count == 0 || plan.step_count == 0 ||
      !saw_run || plan.steps[plan.step_count - 1].kind != SCENARIO_RUN) {
    fail("scenario plan lifecycle is invalid");
  }
  for (size_t index = 0; index < plan.step_count; index++) {
    const struct scenario_step *step = &plan.steps[index];

    if (step->kind == SCENARIO_LOAD) {
      saw_load = 1;
      saw_primary_load |= step->source_index == 0;
    } else if (step->kind == SCENARIO_RESET) {
      if (!saw_load) {
        fail("scenario RESET must follow a LOAD");
      }
      saw_reset = 1;
    } else if (step->kind == SCENARIO_SET_STRATEGY) {
      if (saw_strategy) {
        fail("scenario has multiple SET-STRATEGY steps");
      }
      saw_strategy = 1;
    }
  }
  if (!saw_load || !saw_primary_load || !saw_reset) {
    fail("scenario plan requires primary LOAD and RESET steps");
  }
  return plan;
}

static void release_scenario_plan(struct scenario_plan *plan) {
  if (plan->sources != NULL) {
    (void)memset(plan->sources, 0,
                 MAX_SCENARIO_SOURCES * sizeof(*plan->sources));
    free(plan->sources);
  }
  if (plan->steps != NULL) {
    (void)memset(plan->steps, 0,
                 MAX_SCENARIO_STEPS * sizeof(*plan->steps));
    free(plan->steps);
  }
  plan->sources = NULL;
  plan->source_count = 0;
  plan->total_source_bytes = 0;
  plan->steps = NULL;
  plan->step_count = 0;
}

static void clear_environment_error_state(void *environment) {
  SetEvaluationError(environment, FALSE);
  SetHaltExecution(environment, FALSE);
  EnvSetHaltRules(environment, FALSE);
}

static void clear_observed_run_state(void) {
  observed_halt_rules = 0;
  observed_halt_execution = 0;
  observed_evaluation_error = 0;
}

static void complete_terminal_observation(void) {
  if (!observer_complete) {
    emit_lifecycle(3, "COMPLETE");
    observer_complete = 1;
  }
}

static int execute_load(void *environment, const char *source_path,
                        enum scenario_failure_policy policy) {
  int failed;

  clear_environment_error_state(environment);
  begin_diagnostic_phase("load");
  failed = EnvLoad(environment, source_path) != 1;
  end_diagnostic_phase("load", failed,
                       failed && policy == SCENARIO_CONTINUE);
  if (failed && policy == SCENARIO_STOP) {
    complete_terminal_observation();
    return 0;
  }
  clear_environment_error_state(environment);
  return 1;
}

static int execute_reset(void *environment,
                         enum scenario_failure_policy policy) {
  int evaluation_error;
  int halt_execution;
  int halt_rules;
  int failed;

  clear_environment_error_state(environment);
  begin_diagnostic_phase("reset");
  EnvReset(environment);
  evaluation_error = GetEvaluationError(environment) != 0;
  halt_execution = GetHaltExecution(environment) != 0;
  halt_rules = EnvGetHaltRules(environment) != 0;
  failed = evaluation_error || halt_execution || halt_rules;
  end_diagnostic_phase("reset", failed,
                       failed && policy == SCENARIO_CONTINUE);
  if (failed && policy == SCENARIO_STOP) {
    complete_terminal_observation();
    return 0;
  }
  clear_environment_error_state(environment);
  clear_observed_run_state();
  return 1;
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

static int execute_final_run(void *environment, int probe_after_error) {
  long long rules_fired;
  long long agenda_size;
  int run_failed;

  clear_environment_error_state(environment);
  clear_observed_run_state();
  if (!EnvAddRunFunction(environment, "ferric-native-observer",
                         observe_after_firing, 3000)) {
    fail("cannot register the run observer");
  }
  begin_diagnostic_phase("run");
  rules_fired = EnvRun(environment, -1);
  observe_after_firing(environment);
  if (!EnvRemoveRunFunction(environment, "ferric-native-observer")) {
    fail("cannot remove the run observer");
  }
  run_failed = observed_evaluation_error != 0 ||
               (observed_halt_execution != 0 && observed_halt_rules == 0);
  end_diagnostic_phase("run", run_failed, 0);
  agenda_size = count_agenda(environment);
  emit_formatted_record(
      "RUN|-1|%lld|%d|%d|%d|%lld|%d", rules_fired,
      observed_halt_rules != 0, observed_halt_execution != 0,
      observed_evaluation_error != 0, agenda_size, protocol_violation != 0);
  observer_after_run = 1;

  if (run_failed && !probe_after_error) {
    complete_terminal_observation();
  } else {
    if (run_failed) {
      /* Preserve observed_* for the already emitted RUN record while making
         the environment safe for trusted, post-run, read-only probes. */
      clear_environment_error_state(environment);
    }
    evaluate_probe_operations(environment);
    if (!observer_complete) {
      mark_violation("completion-missing");
    }
  }
  return protocol_violation ? 1 : 0;
}

int main(int argc, char **argv) {
  char raw_config[MAX_CONFIG_LENGTH + 1];
  const char *input_path;
  struct scenario_plan scenario = {NULL, 0, 0, NULL, 0};
  void *environment;
  int scenario_mode;
  int result = 0;

  if (argc != 3 ||
      (strcmp(argv[1], "--source") != 0 &&
       strcmp(argv[1], "--scenario") != 0)) {
    fprintf(stderr,
            "usage: ferric-clips-observer "
            "--source|--scenario <repository-path>\n");
    return 127;
  }
  scenario_mode = strcmp(argv[1], "--scenario") == 0;
  input_path = argv[2];
  const size_t config_length =
      read_config_line(raw_config, sizeof(raw_config) - 1);
  raw_config[config_length] = '\0';
  parse_config(raw_config);
  (void)memset(raw_config, 0, sizeof(raw_config));
  if (scenario_mode) {
    scenario = parse_scenario_plan(input_path);
  }

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
  if (!EnvAddRouter(environment, DIAGNOSTIC_ROUTER_NAME, 30,
                    diagnostic_router_query, diagnostic_router_print, NULL,
                    NULL, NULL)) {
    fprintf(stderr,
            "ferric CLIPS observer: cannot register diagnostic router\n");
    (void)DestroyEnvironment(environment);
    return 127;
  }

  emit_lifecycle(0, "START");
  if (!scenario_mode) {
    if (!execute_load(environment, input_path, SCENARIO_STOP)) {
      result = 1;
    } else {
      (void)execute_reset(environment, SCENARIO_CONTINUE);
      result = execute_final_run(environment, 0);
    }
  } else {
    for (size_t index = 0; index < scenario.step_count; index++) {
      const struct scenario_step *step = &scenario.steps[index];

      if (step->kind == SCENARIO_LOAD) {
        const char *source_path =
            scenario.sources[step->source_index].resolved_path;
        if (!execute_load(environment, source_path, step->policy)) {
          result = 1;
          break;
        }
      } else if (step->kind == SCENARIO_RESET) {
        if (!execute_reset(environment, step->policy)) {
          result = 1;
          break;
        }
      } else if (step->kind == SCENARIO_SET_STRATEGY) {
        (void)EnvSetStrategy(environment, step->strategy);
      } else {
        result = execute_final_run(environment, 1);
      }
    }
  }
  if (!DestroyEnvironment(environment)) {
    fprintf(stderr, "ferric CLIPS observer: cannot destroy environment\n");
    result = 127;
  }
  observer_environment = NULL;
  release_diagnostic_output();
  release_scenario_plan(&scenario);
  (void)memset(&config, 0, sizeof(config));
  return result;
}
