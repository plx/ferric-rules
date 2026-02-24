#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME="ferric-rules/clips-reference"
IMAGE_TAG="latest"
PLATFORMS="linux/amd64,linux/arm64"
LOAD_LOCAL=0
CLIPS_FILES=()
OPS=()
OPS_FILE=""
WORKDIR_IN_CONTAINER="/workspace"

usage() {
  cat <<'USAGE'
Usage:
  scripts/clips-reference.sh build [options]
  scripts/clips-reference.sh run [options]

Commands:
  build                 Build a multi-platform CLIPS image.
  run                   Start CLIPS in Docker and execute files/operations.

Build options:
  --image <name>        Docker image name (default: ferric-rules/clips-reference)
  --tag <tag>           Docker image tag (default: latest)
  --platforms <list>    Platforms for buildx (default: linux/amd64,linux/arm64)
  --load                Load single-platform image into local Docker daemon
                        (forces platform linux/amd64 for local testing)

Run options:
  --image <name>        Docker image name (default: ferric-rules/clips-reference)
  --tag <tag>           Docker image tag (default: latest)
  --file <path>         CLIPS source file to batch* load (repeatable)
  --ops-file <path>     Text file containing CLIPS expressions (one per line)
  --op <expr>           CLIPS expression to execute (repeatable)

Examples:
  scripts/clips-reference.sh build
  scripts/clips-reference.sh build --load
  scripts/clips-reference.sh run --file examples/rules.clp --op '(reset)' --op '(run)'
  scripts/clips-reference.sh run --file a.clp --file b.clp --ops-file scripts/sequence.clp
USAGE
}

require_command() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "error: required command not found: $cmd" >&2
    exit 1
  fi
}

resolve_path() {
  local input="$1"
  if [[ -f "$input" ]]; then
    echo "$(cd "$(dirname "$input")" && pwd)/$(basename "$input")"
    return
  fi

  echo "error: file not found: $input" >&2
  exit 1
}

build_command() {
  require_command docker

  local full_image="${IMAGE_NAME}:${IMAGE_TAG}"
  if [[ "$LOAD_LOCAL" -eq 1 ]]; then
    local host_arch
    host_arch="$(uname -m)"
    case "$host_arch" in
      arm64|aarch64) host_arch="linux/arm64" ;;
      x86_64|amd64)  host_arch="linux/amd64" ;;
      *)             host_arch="linux/amd64" ;;
    esac
    echo "Building local image ${full_image} for ${host_arch} (--load)."
    docker buildx build \
      --platform "$host_arch" \
      --load \
      -t "$full_image" \
      docker/clips-reference
  else
    echo "Building multi-platform image ${full_image} for ${PLATFORMS}."
    echo "Note: this uses --push so the result is available for both architectures."
    docker buildx build \
      --platform "$PLATFORMS" \
      --push \
      -t "$full_image" \
      docker/clips-reference
  fi
}

run_command() {
  require_command docker

  local full_image="${IMAGE_NAME}:${IMAGE_TAG}"
  local repo_root
  repo_root="$(git rev-parse --show-toplevel)"

  local commands=()
  local file
  for file in "${CLIPS_FILES[@]}"; do
    local abs
    abs="$(resolve_path "$file")"
    if [[ "$abs" != "$repo_root"/* ]]; then
      echo "error: --file path must be inside repository: $file" >&2
      exit 1
    fi

    local rel="${abs#${repo_root}/}"
    commands+=("(batch* \"${WORKDIR_IN_CONTAINER}/${rel}\")")
  done

  if [[ -n "$OPS_FILE" ]]; then
    local abs_ops
    abs_ops="$(resolve_path "$OPS_FILE")"
    while IFS= read -r line || [[ -n "$line" ]]; do
      [[ -z "$line" ]] && continue
      commands+=("$line")
    done < "$abs_ops"
  fi

  local op
  for op in "${OPS[@]}"; do
    commands+=("$op")
  done

  if [[ "${#commands[@]}" -eq 0 ]]; then
    commands+=("(reset)" "(run)")
  fi

  {
    for cmd in "${commands[@]}"; do
      printf '%s\n' "$cmd"
    done
    printf '(exit)\n'
  } | docker run --rm -i \
      -v "${repo_root}:${WORKDIR_IN_CONTAINER}" \
      -w "$WORKDIR_IN_CONTAINER" \
      "$full_image"
}

[[ $# -eq 0 ]] && { usage; exit 1; }

COMMAND="$1"
shift

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image)
      IMAGE_NAME="$2"
      shift 2
      ;;
    --tag)
      IMAGE_TAG="$2"
      shift 2
      ;;
    --platforms)
      PLATFORMS="$2"
      shift 2
      ;;
    --load)
      LOAD_LOCAL=1
      shift
      ;;
    --file)
      CLIPS_FILES+=("$2")
      shift 2
      ;;
    --ops-file)
      OPS_FILE="$2"
      shift 2
      ;;
    --op)
      OPS+=("$2")
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

case "$COMMAND" in
  build)
    build_command
    ;;
  run)
    run_command
    ;;
  *)
    echo "error: unknown command: $COMMAND" >&2
    usage
    exit 1
    ;;
esac
