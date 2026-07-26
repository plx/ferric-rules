#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define CLIPS_PATH "/usr/bin/clips"
#define OBSERVER_PATH "/usr/local/bin/ferric-clips-observer"
#define OBSERVER_FLAG "--ferric-observer"

static void fail(const char *message) {
  fprintf(stderr, "ferric CLIPS launcher: %s\n", message);
  exit(127);
}

int main(int argc, char **argv) {
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
