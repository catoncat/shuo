#!/bin/sh
set -eu

prefix=""
for candidate in /opt/homebrew /usr/local; do
    if [ -f "$candidate/lib/pkgconfig/opus.pc" ]; then
        prefix="$candidate"
        break
    fi
done

if [ -z "$prefix" ]; then
    exit 1
fi

libdir="$prefix/lib"
includedir="$prefix/include"
modversion=$(sed -n 's/^Version:[[:space:]]*//p' "$prefix/lib/pkgconfig/opus.pc" | head -n 1)
modversion=${modversion:-1.0.0}

want_exists=0
want_modversion=0
want_libs=0
want_libs_only_l=0
want_libs_only_L=0
want_cflags=0
want_cflags_only_I=0

for arg in "$@"; do
    case "$arg" in
        --exists)
            want_exists=1
            ;;
        --modversion)
            want_modversion=1
            ;;
        --libs|--static)
            want_libs=1
            ;;
        --libs-only-l)
            want_libs_only_l=1
            ;;
        --libs-only-L)
            want_libs_only_L=1
            ;;
        --cflags)
            want_cflags=1
            ;;
        --cflags-only-I)
            want_cflags_only_I=1
            ;;
        opus|--print-errors|--short-errors|--silence-errors)
            ;;
    esac
done

if [ "$want_exists" -eq 1 ]; then
    exit 0
fi

if [ "$want_modversion" -eq 1 ]; then
    printf '%s\n' "$modversion"
    exit 0
fi

if [ "$want_libs_only_L" -eq 1 ]; then
    printf '%s\n' "-L$libdir"
    exit 0
fi

if [ "$want_libs_only_l" -eq 1 ]; then
    printf '%s\n' "-lopus"
    exit 0
fi

if [ "$want_cflags_only_I" -eq 1 ]; then
    printf '%s\n' "-I$includedir"
    exit 0
fi

parts=""
if [ "$want_cflags" -eq 1 ]; then
    parts="-I$includedir"
fi
if [ "$want_libs" -eq 1 ]; then
    parts="${parts:+$parts }-L$libdir -lopus"
fi
printf '%s\n' "$parts"
