#!/usr/bin/env bash
# Headless smoke test for the tenon plugin's Lua-facing module contract.
# Builds the plugin, then runs each Lua check phase in a fresh nvim --headless
# process. Exits nonzero if any phase fails.
set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "==> cargo build --release"
cargo build --release || exit 1

if [[ ! -e lua/tenon.so && ! -e lua/tenon/core.so ]]; then
    echo "error: no build artifact found (expected lua/tenon.so or lua/tenon/core.so)" >&2
    exit 1
fi

PHASES=(baseline gate-closed setup-error gate-open)
overall=0
for phase in "${PHASES[@]}"; do
    echo "==> phase: $phase"
    nvim --headless -u NONE \
        --cmd "set runtimepath^=$REPO_ROOT" \
        +"lua _G.SMOKE_PHASE='$phase'" \
        +"lua local ok, err = pcall(dofile, '$REPO_ROOT/scripts/smoke-test.lua') \
             if not ok then print('HARNESS ERROR: ' .. tostring(err)) vim.cmd('cq 99') end"
    status=$?
    # nvim --headless emits no trailing newline after its last message;
    # end each phase's output on its own line.
    echo
    if (( status != 0 )); then
        overall=$status
    fi
done

if (( overall != 0 )); then
    echo "SMOKE TEST FAILED (exit $overall)"
else
    echo "SMOKE TEST PASSED"
fi
exit $overall
