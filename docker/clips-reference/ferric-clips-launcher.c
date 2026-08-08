#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define CLIPS_PATH "/usr/bin/clips"
#define OBSERVER_PATH "/usr/local/bin/ferric-clips-observer"
#define OBSERVER_FLAG "--ferric-observer"
#define PROVENANCE_FLAG "--ferric-provenance"
#define PROVENANCE_PATH "/usr/local/share/ferric-clips-reference/provenance"
#define PROVENANCE_PREFIX "FERRIC-CLIPS-PROVENANCE|1|"

static void fail(const char *message) {
  fprintf(stderr, "ferric CLIPS launcher: %s\n", message);
  exit(127);
}

static void emit_provenance(void) {
  char buffer[1024];
  FILE *file = fopen(PROVENANCE_PATH, "rb");
  size_t length;
  size_t newline_count = 0;

  if (file == NULL) {
    fail("could not open reference provenance");
  }
  length = fread(buffer, 1, sizeof(buffer), file);
  if (ferror(file) || !feof(file) || fclose(file) != 0) {
    fail("could not read reference provenance");
  }
  if (length <= sizeof(PROVENANCE_PREFIX) || length == sizeof(buffer) ||
      buffer[length - 1] != '\n' ||
      memcmp(buffer, PROVENANCE_PREFIX, sizeof(PROVENANCE_PREFIX) - 1) != 0) {
    fail("reference provenance is malformed");
  }
  for (size_t index = 0; index < length; index++) {
    const unsigned char character = (unsigned char)buffer[index];
    if (character == '\n') {
      newline_count++;
    } else if (character < 0x20U || character == 0x7fU) {
      fail("reference provenance contains control characters");
    }
  }
  if (newline_count != 1) {
    fail("reference provenance must contain exactly one line");
  }
  if (fwrite(buffer, 1, length, stdout) != length || fflush(stdout) != 0) {
    fail("could not emit reference provenance");
  }
}

int main(int argc, char **argv) {
  if (argc > 1 && strcmp(argv[1], PROVENANCE_FLAG) == 0) {
    if (argc != 2) {
      fail("provenance mode does not accept arguments");
    }
    emit_provenance();
    return 0;
  }
  const int observer_mode = argc > 1 && strcmp(argv[1], OBSERVER_FLAG) == 0;
  const char *program = observer_mode ? OBSERVER_PATH : CLIPS_PATH;
  const int program_argc = observer_mode ? argc - 1 : argc;
  char **program_argv =
      calloc((size_t)program_argc + 1, sizeof(*program_argv));

  if (program_argv == NULL) {
    fail("could not allocate process arguments");
  }
  program_argv[0] = (char *)program;
  for (int index = 1; index < program_argc; index++) {
    program_argv[index] = argv[index + (observer_mode ? 1 : 0)];
  }
  program_argv[program_argc] = NULL;
  execv(program, program_argv);
  fail(observer_mode ? "could not execute the structured observer"
                     : "could not execute CLIPS");
}
