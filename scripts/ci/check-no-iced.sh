#!/usr/bin/env bash
set -euo pipefail

if rg -n '\biced\b|iced_' Cargo.toml Cargo.lock src build.rs; then
    echo 'iced references remain in production code or dependencies' >&2
    exit 1
fi
