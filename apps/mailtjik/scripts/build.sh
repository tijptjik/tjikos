#!/usr/bin/env bash
set -euo pipefail
mkdir -p dist/backend
go build -buildvcs=false -trimpath -o dist/mailtjik ./cmd/mailtjik
cp backend/*.py dist/backend/
