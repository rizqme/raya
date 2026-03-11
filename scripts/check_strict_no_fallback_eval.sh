#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

extract_first_ir_module() {
  local in_file="$1"
  awk '
    BEGIN { seen = 0 }
    /RAYA_DEBUG_DUMP_IR/ {
      seen++
      if (seen == 2) exit
    }
    { print }
  ' "$in_file"
}

run_ok_no_fallback() {
  local name="$1"
  local source="$2"
  local out_file
  out_file="$(mktemp)"

  if ! RAYA_DEBUG_DUMP_IR=1 cargo run -q -p raya-cli -- eval "$source" >"$out_file" 2>&1; then
    echo "[FAIL] $name: eval failed"
    sed -n '1,40p' "$out_file"
    return 1
  fi

  local first_ir
  first_ir="$(mktemp)"
  extract_first_ir_module "$out_file" >"$first_ir"

  local late_bound_count json_fallback_count
  late_bound_count="$( (rg -n "late_bound" "$first_ir" -S || true) | wc -l | tr -d ' ' )"
  json_fallback_count="$( (rg -n "json_get|json_set" "$first_ir" -S || true) | wc -l | tr -d ' ' )"

  if [[ "$late_bound_count" != "0" || "$json_fallback_count" != "0" ]]; then
    echo "[FAIL] $name: strict source IR contains fallback ops"
    echo "late_bound=$late_bound_count json_get_or_set=$json_fallback_count"
    rg -n "late_bound|json_get|json_set" "$first_ir" -S | sed -n '1,60p'
    return 1
  fi

  echo "[PASS] $name"
}

run_err() {
  local name="$1"
  local source="$2"
  local expected="$3"
  local out_file
  out_file="$(mktemp)"

  if cargo run -q -p raya-cli -- eval "$source" >"$out_file" 2>&1; then
    echo "[FAIL] $name: expected failure, got success"
    return 1
  fi

  if ! rg -q "$expected" "$out_file" -S; then
    echo "[FAIL] $name: expected pattern '$expected' not found"
    sed -n '1,40p' "$out_file"
    return 1
  fi

  echo "[PASS] $name"
}

echo "Running strict no-fallback eval audit..."

run_ok_no_fallback "class-method-dispatch" \
  'class A { x: number; constructor(){ this.x = 1; } getX(): number { return this.x; } } let a = new A(); return a.getX();'

run_ok_no_fallback "structural-width-subset" \
  'type A = { a: number, b: string }; type B = { a: number, b: string, c: string }; let x: B = { a: 1, b: "s", c: "t" }; let y: A = x; return y.a;'

run_ok_no_fallback "discriminated-union-narrowing" \
  'type U = { kind: "a", a: number } | { kind: "b", b: number }; let u: U = { kind: "a", a: 5 }; if (u.kind == "a") { return u.a; } return 0;'

run_ok_no_fallback "generic-constraint-member-call" \
  'type HasM = { m: () => number }; function f<T extends HasM>(x: T): number { return x.m(); } let v = { m: (): number => 7 }; return f(v);'

run_ok_no_fallback "generic-map-null-narrowing" \
  'class C<T> { m: Map<string, T[]>; constructor(){ this.m = new Map<string, T[]>(); } f(k: string): number { let g = this.m.get(k); if (g != null) { return g.length; } return 0; } } let c = new C<number>(); return c.f("x");'

run_err "unknown-member-rejected" \
  'let u: unknown = { m: (): number => 1 }; return u.m();' \
  'E_STRICT_UNKNOWN_NOT_ACTIONABLE'

run_err "unknown-binary-rejected" \
  'let u: unknown = 1; return u + 1;' \
  'E_STRICT_UNKNOWN_NOT_ACTIONABLE'

echo "Strict no-fallback eval audit passed."
