Scripts that generate binary fixtures. Outputs are committed so tests never
need these tools.

- `make-pe-stub.sh`: a resource-only PE (no code, no entry point) with a
  version resource, ~1.5 KB. Used wherever a scanner reads a Windows binary's
  version info. Requires `llvm-rc` and `lld-link`.
