#!/usr/bin/env bash
set -euo pipefail
prompt="$(cat)"
gemini -y -p "$prompt" --output-format text
