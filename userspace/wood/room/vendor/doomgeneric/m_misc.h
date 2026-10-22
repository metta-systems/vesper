//
// Copyright(C) 1993-1996 Id Software, Inc.
// Copyright(C) 2005-2014 Simon Howard
//
// This program is free software; you can redistribute it and/or
// modify it under the terms of the GNU General Public License
// as published by the Free Software Foundation; either version 2
// of the License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// DESCRIPTION:
//      Miscellaneous.
//    


#ifndef __M_MISC__
#define __M_MISC__

#include <stdio.h>
#include <stdarg.h>

#include "doomtype.h"

boolean M_WriteFile(char *name, void *source, int length);
int M_ReadFile(char *name, byte **buffer);
void M_MakeDirectory(char *dir);
char *M_TempFile(char *s);
boolean M_FileExists(char *file);
long M_FileLength(FILE *handle);
boolean M_StrToInt(const char *str, int *result);
void M_ExtractFileBase(char *path, char *dest);
void M_ForceUppercase(char *text);
char *M_StrCaseStr(char *haystack, char *needle);
char *M_StringDuplicate(const char *orig);
boolean M_StringCopy(char *dest, const char *src, size_t dest_size);
boolean M_StringConcat(char *dest, const char *src, size_t dest_size);
char *M_StringReplace(const char *haystack, const char *needle,
                      const char *replacement);
char *M_StringJoin(const char *s, ...);
boolean M_StringStartsWith(const char *s, const char *prefix);
boolean M_StringEndsWith(const char *s, const char *suffix);
int M_vsnprintf(char *buf, size_t buf_len, const char *s, va_list args);
int M_snprintf(char *buf, size_t buf_len, const char *s, ...);
char *M_OEMToUTF8(const char *ansi);

// Non-variadic helper implemented in Rust.
// `strs` is a NULL-terminated array of `const char *`.
char *M_StringJoinA(const char *const *strs);

// Trailing-clamp helper used by the M_snprintf / M_vsnprintf macros.
// NOTE: M_snprintf macro evaluates `buf` and `len` twice. All current
// call-sites pass plain lvalues, so this is safe.
int M_snprintf_clamp(char *buf, size_t len, int result);

// Macro replacements that redirect C callers to non-variadic helpers.
// Rust callers cannot use these macros and must call snprintf/M_StringJoinA directly.
#define M_StringJoin(...)                                              \
    M_StringJoinA((const char *const[]){ __VA_ARGS__ })

#define M_snprintf(buf, len, ...)                                      \
    M_snprintf_clamp((buf), (len), snprintf((buf), (len), __VA_ARGS__))

#define M_vsnprintf(buf, len, fmt, ap)                                 \
    M_snprintf_clamp((buf), (len), vsnprintf((buf), (len), (fmt), (ap)))

#endif

