#!/usr/bin/env bash
# Build a tiny resource-only PE (no code, no entry point) carrying a version
# resource. Stands in for a plug-in binary in scanner fixtures.
#
# Usage: make-pe-stub.sh <out> <x64|arm64|x86> <ProductName> <CompanyName> <ProductVersion> <FileVersion> [lang-hex]
# Needs llvm-rc and lld-link (LLVM). Commit the output; tests don't run this.
set -euo pipefail
out=$1 machine=$2 product=$3 company=$4 pver=$5 fver=$6 lang=${7:-0409}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
IFS=. read -r a b c d _ <<<"$fver.0.0.0.0"
cat >"$tmp/v.rc" <<RC
1 VERSIONINFO
FILEVERSION $a,$b,$c,$d
PRODUCTVERSION $a,$b,$c,$d
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "${lang}04B0"
    BEGIN
      VALUE "CompanyName", "$company"
      VALUE "ProductName", "$product"
      VALUE "FileVersion", "$fver"
      VALUE "ProductVersion", "$pver"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x$lang, 1200
  END
END
RC
llvm-rc /fo "$tmp/v.res" "$tmp/v.rc"
mkdir -p "$(dirname "$out")"
lld-link /nologo /dll /noentry "/machine:$machine" "$tmp/v.res" "/out:$out"
rm -f "${out%.*}.lib"
