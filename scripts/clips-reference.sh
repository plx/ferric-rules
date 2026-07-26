#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME="ferric-rules/clips-reference"
IMAGE_TAG="latest"
PLATFORMS="linux/amd64,linux/arm64"
LOAD_LOCAL=0
QUIET=0
CLIPS_FILES=()
OPS=()
OPS_FILE=""
OBSERVER_NONCE=""
OBSERVER_FIXTURE_ID=""
OBSERVER_SOURCE_SHA256=""
OBSERVER_COMPOSED_SHA256=""
OBSERVER_AUTH_KEY=""
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
  --observer-nonce <n>  Enable nonce-bound native run metadata (internal)
  --observer-fixture-id <id>
                        Bind native observations to a fixture (internal)
  --observer-source-sha256 <digest>
                        Bind native observations to source bytes (internal)
  --observer-composed-sha256 <digest>
                        Bind native observations to composed bytes (internal)
  --observer-auth-key <key>
                        Authenticate native observation records (internal)
  --quiet               Execute stdin with CLIPS batch* semantics, suppressing
                        the interactive banner, prompts, and return values

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
  if [[ ! -f "$input" ]]; then
    echo "error: file not found: $input" >&2
    exit 1
  fi

  if [[ -L "$input" ]]; then
    echo "error: file path must not be a symlink: $input" >&2
    exit 1
  fi

  local directory
  local filename
  local physical_directory
  directory="$(dirname "$input")"
  filename="$(basename "$input")"
  physical_directory="$(cd "$directory" && pwd -P)"
  printf '%s/%s\n' "$physical_directory" "$filename"
}

escape_clips_string() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  printf '%s' "$value"
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
  repo_root="$(cd "$repo_root" && pwd -P)"

  local commands=()
  local container_files=()
  local file
  for file in ${CLIPS_FILES[@]+"${CLIPS_FILES[@]}"}; do
    local abs
    abs="$(resolve_path "$file")"
    if [[ "$abs" != "$repo_root"/* ]]; then
      echo "error: --file path must be inside repository: $file" >&2
      exit 1
    fi

    local rel="${abs:$(( ${#repo_root} + 1 ))}"
    if [[ "$rel" =~ [[:cntrl:]] ]]; then
      echo "error: --file path contains an unsupported control character: $file" >&2
      exit 1
    fi
    local container_path
    container_path="$(escape_clips_string "${WORKDIR_IN_CONTAINER}/${rel}")"
    container_files+=("${WORKDIR_IN_CONTAINER}/${rel}")
    if [[ -z "$OBSERVER_NONCE" ]]; then
      commands+=("(batch* \"${container_path}\")")
    fi
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
  for op in ${OPS[@]+"${OPS[@]}"}; do
    commands+=("$op")
  done

  if [[ -n "$OBSERVER_NONCE" ]]; then
    if [[ "${#container_files[@]}" -ne 1 ]]; then
      echo "error: structured observer requires exactly one --file" >&2
      exit 1
    fi
  elif [[ "${#commands[@]}" -eq 0 ]]; then
    commands+=("(reset)" "(run)")
  elif [[ "${#OPS[@]}" -eq 0 && -z "$OPS_FILE" ]]; then
    # When only --file was given (no explicit --op or --ops-file),
    # append (reset)(run) to execute rules after loading.  This matches
    # ferric's "load → reset → run" behavior.
    commands+=("(reset)" "(run)")
  fi

  local clips_args=()
  local observer_args=()
  if [[ "$QUIET" -eq 1 && -z "$OBSERVER_NONCE" ]]; then
    clips_args+=("-f2" "/dev/stdin")
  fi
  if [[ -n "$OBSERVER_NONCE" ]]; then
    if [[ ! "$OBSERVER_NONCE" =~ ^[0-9a-f]{32,128}$ ]] ||
       (( ${#OBSERVER_NONCE} % 2 != 0 )); then
      echo "error: --observer-nonce must encode 16-64 bytes as lowercase hexadecimal" >&2
      exit 1
    fi
    if [[ ! "$OBSERVER_FIXTURE_ID" =~ ^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$ ]]; then
      echo "error: --observer-fixture-id must be a protocol-safe token" >&2
      exit 1
    fi
    if [[ ! "$OBSERVER_SOURCE_SHA256" =~ ^[0-9a-f]{64}$ ]]; then
      echo "error: --observer-source-sha256 must be a lowercase SHA-256 digest" >&2
      exit 1
    fi
    if [[ ! "$OBSERVER_COMPOSED_SHA256" =~ ^[0-9a-f]{64}$ ]]; then
      echo "error: --observer-composed-sha256 must be a lowercase SHA-256 digest" >&2
      exit 1
    fi
    if [[ ! "$OBSERVER_AUTH_KEY" =~ ^[0-9a-f]{64}$ ]]; then
      echo "error: --observer-auth-key must encode 32 bytes as lowercase hexadecimal" >&2
      exit 1
    fi
    observer_args+=(
      "--ferric-observer"
      "--source"
      "${container_files[0]}"
    )
  elif [[ -n "$OBSERVER_FIXTURE_ID" || -n "$OBSERVER_SOURCE_SHA256" ||
          -n "$OBSERVER_COMPOSED_SHA256" || -n "$OBSERVER_AUTH_KEY" ]]; then
    echo "error: observer bindings require --observer-nonce" >&2
    exit 1
  fi

  {
    if [[ -n "$OBSERVER_NONCE" ]]; then
      printf '%s|%s|%s|%s|%s\n' \
        "$OBSERVER_NONCE" \
        "$OBSERVER_FIXTURE_ID" \
        "$OBSERVER_SOURCE_SHA256" \
        "$OBSERVER_COMPOSED_SHA256" \
        "$OBSERVER_AUTH_KEY"
    fi
    for cmd in ${commands[@]+"${commands[@]}"}; do
      printf '%s\n' "$cmd"
    done
    if [[ -z "$OBSERVER_NONCE" ]]; then
      printf '(exit)\n'
    fi
  } | docker run --rm -i \
      -v "${repo_root}:${WORKDIR_IN_CONTAINER}:ro" \
      -w "$WORKDIR_IN_CONTAINER" \
      "$full_image" \
      ${observer_args[@]+"${observer_args[@]}"} \
      ${clips_args[@]+"${clips_args[@]}"}
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
    --observer-nonce)
      OBSERVER_NONCE="$2"
      shift 2
      ;;
    --observer-fixture-id)
      OBSERVER_FIXTURE_ID="$2"
      shift 2
      ;;
    --observer-source-sha256)
      OBSERVER_SOURCE_SHA256="$2"
      shift 2
      ;;
    --observer-composed-sha256)
      OBSERVER_COMPOSED_SHA256="$2"
      shift 2
      ;;
    --observer-auth-key)
      OBSERVER_AUTH_KEY="$2"
      shift 2
      ;;
    --quiet)
      QUIET=1
      shift
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
