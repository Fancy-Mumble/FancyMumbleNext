#!/usr/bin/env bash
# check-android-so.sh - Verify Android .so files have no unresolved C++ runtime symbols.
#
# The Rust linker for Android uses NDK clang in C mode, which does NOT
# auto-link libc++.  If a crate depends on a C++ library (e.g. oboe),
# build.rs must explicitly pass -lc++_shared.  When this is missing,
# the resulting .so contains unresolved symbols like __cxa_pure_virtual
# and the app crashes at startup with:
#
#   dlopen failed: cannot locate symbol "__cxa_pure_virtual"
#
# This script catches that at build time by inspecting the dynamic
# symbol table with readelf.
#
# Usage:
#   ./scripts/check-android-so.sh                                # auto-discover
#   ./scripts/check-android-so.sh path/to/libmumble_tauri_lib.so
#
# Requirements:
#   readelf or llvm-readelf (pre-installed on ubuntu runners)

set -euo pipefail

# C++ runtime symbols that MUST NOT be undefined (UND) in the .so.
# These come from libc++ and indicate the C++ runtime is not linked.
# Excludes __cxa_finalize/__cxa_atexit which live in libc (always UND).
FORBIDDEN_UND=(
    __cxa_pure_virtual
    __cxa_deleted_virtual
    __cxa_guard_acquire
    __cxa_guard_release
    __cxa_guard_abort
    __gxx_personality_v0
)

# --- Resolve readelf binary ---

find_readelf() {
    if command -v llvm-readelf &>/dev/null; then
        echo "llvm-readelf"
    elif command -v readelf &>/dev/null; then
        echo "readelf"
    elif [[ -n "${NDK_HOME:-}" ]]; then
        local ndk_readelf
        # Linux CI
        ndk_readelf="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf"
        if [[ -x "$ndk_readelf" ]]; then
            echo "$ndk_readelf"
            return
        fi
        # Windows (Git Bash / MSYS2)
        ndk_readelf="$NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-readelf.exe"
        if [[ -x "$ndk_readelf" ]]; then
            echo "$ndk_readelf"
            return
        fi
    fi
}

READELF=$(find_readelf)
if [[ -z "$READELF" ]]; then
    echo "ERROR: Neither readelf nor llvm-readelf found."
    echo "       Install binutils or set NDK_HOME."
    exit 1
fi

# --- Collect .so files to check ---

SO_FILES=()

if [[ $# -gt 0 ]]; then
    # Explicit paths provided
    for arg in "$@"; do
        if [[ -f "$arg" ]]; then
            SO_FILES+=("$arg")
        else
            echo "ERROR: File not found: $arg"
            exit 1
        fi
    done
else
    # Auto-discover from cargo build output. The crate's [lib] name is
    # `mumble_tauri_lib`, so cargo emits `libmumble_tauri_lib.so` (NOT
    # `libmumble_tauri.so`).
    for triple in aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android; do
        for profile in debug release; do
            so="target/${triple}/${profile}/libmumble_tauri_lib.so"
            if [[ -f "$so" ]]; then
                SO_FILES+=("$so")
            fi
        done
    done
fi

if [[ ${#SO_FILES[@]} -eq 0 ]]; then
    echo "ERROR: No libmumble_tauri_lib.so files found."
    echo "       Build for Android first, or pass an explicit path."
    echo ""
    echo "Usage: $0 [path/to/libmumble_tauri_lib.so ...]"
    exit 1
fi

# --- Check each .so ---

OVERALL_PASS=0

for so in "${SO_FILES[@]}"; do
    echo "Checking: $so"
    echo "  Using: $READELF"

    # Extract undefined dynamic symbols
    UND_SYMS=$("$READELF" --dyn-syms "$so" 2>/dev/null | grep ' UND ' || true)

    # Check if libc++_shared.so is a NEEDED dependency (dynamic linkage).
    # When it is, the forbidden symbols are expected to be UND - they will
    # resolve at runtime from the bundled libc++_shared.so.
    HAS_LIBCXX_NEEDED=$("$READELF" -d "$so" 2>/dev/null | grep 'NEEDED.*libc++_shared' || true)

    FILE_FAIL=0
    if [[ -z "$HAS_LIBCXX_NEEDED" ]]; then
        # No dynamic C++ runtime linked - symbols must NOT be UND
        for sym in "${FORBIDDEN_UND[@]}"; do
            if echo "$UND_SYMS" | grep -qw "$sym"; then
                echo "  FAIL: '$sym' is an undefined dynamic symbol (no libc++_shared NEEDED)"
                FILE_FAIL=1
            fi
        done
    else
        echo "  libc++_shared.so is a NEEDED dependency (OK)"
    fi

    if [[ $FILE_FAIL -ne 0 ]]; then
        echo "  -> FAILED: unresolved C++ runtime symbols detected."
        echo "     The app will crash at startup with:"
        echo "       dlopen failed: cannot locate symbol \"...\""
        echo "     Fix: ensure build.rs links c++_shared for Android."
        OVERALL_PASS=1
    else
        echo "  -> PASS (symbols)"
    fi

    # Check that libc++_shared.so is bundled alongside libmumble_tauri.so.
    # Since we dynamically link the C++ runtime, it must be in the APK.
    SO_DIR=$(dirname "$so")
    LIBCXX="$SO_DIR/libc++_shared.so"
    # Also check the Gradle jniLibs dir (used by Tauri CLI)
    JNILIBS_DIR=""
    for abi in arm64-v8a armeabi-v7a x86_64 x86; do
        candidate=$(echo "$so" | grep -o ".*$abi" 2>/dev/null || true)
        if [[ -n "$candidate" ]]; then
            JNILIBS_DIR="$candidate"
            break
        fi
    done

    if [[ -f "$LIBCXX" ]]; then
        echo "  -> PASS (libc++_shared.so bundled)"
    elif [[ -n "$JNILIBS_DIR" && -f "$JNILIBS_DIR/libc++_shared.so" ]]; then
        echo "  -> PASS (libc++_shared.so in jniLibs)"
    else
        # Only warn if the .so actually needs libc++_shared (has NEEDED entry)
        NEEDED=$("$READELF" -d "$so" 2>/dev/null | grep 'NEEDED.*libc++_shared' || true)
        if [[ -n "$NEEDED" ]]; then
            echo "  -> WARNING: libc++_shared.so not found alongside $so"
            echo "     The APK may crash if it is not bundled by the build system."
        fi
    fi
    echo ""
done

if [[ $OVERALL_PASS -ne 0 ]]; then
    echo "RESULT: FAILED - one or more .so files have unresolved C++ symbols."
    exit 1
fi

echo "RESULT: PASSED - all .so files are clean."
