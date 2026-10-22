#!/usr/bin/env python3
"""Rewrite raw integer state indices in info.rs to named S_* constants."""

import re
import sys

INFO_H = "vendor/doomgeneric/info.h"
INFO_RS = "room/src/doom/info.rs"

def parse_enum():
    """Parse statenum_t enum from info.h and return {index: name}."""
    with open(INFO_H) as f:
        content = f.read()
    in_enum = False
    entries = []
    for line in content.splitlines():
        trimmed = line.strip()
        if trimmed.startswith("typedef enum"):
            in_enum = True
            continue
        if in_enum:
            if trimmed.startswith("} statenum_t"):
                break
            if trimmed.startswith("S_"):
                name = trimmed.rstrip(",")
                entries.append(name)
    return {i: name for i, name in enumerate(entries)}

def rewrite_file(mapping):
    """Rewrite info.rs: replace integer state indices with S_* constants."""
    with open(INFO_RS) as f:
        content = f.read()
    
    lines = content.splitlines()
    new_lines = []
    
    for line in lines:
        # Match nextstate: N, in states[]
        m = re.match(r'(\s*nextstate:\s+)(\d+)(\s*,)', line)
        if m:
            idx = int(m.group(2))
            if idx in mapping:
                line = f"{m.group(1)}{mapping[idx]}{m.group(3)}"
            new_lines.append(line)
            continue
        
        # Match mobjinfo state fields: spawnstate, seestate, painstate,
        # meleestate, missilestate, deathstate, xdeathstate, raisestate
        m = re.match(
            r'(\s*(?:spawnstate|seestate|painstate|meleestate|missilestate|deathstate|xdeathstate|raisestate):\s+)(\d+)(\s*,)',
            line,
        )
        if m:
            idx = int(m.group(2))
            if idx in mapping:
                line = f"{m.group(1)}{mapping[idx]}{m.group(3)}"
            new_lines.append(line)
            continue
        
        new_lines.append(line)
    
    with open(INFO_RS, "w") as f:
        f.write("\n".join(new_lines))
    
    print(f"Rewrote {INFO_RS} with {len(mapping)} state constants")

if __name__ == "__main__":
    mapping = parse_enum()
    print(f"Parsed {len(mapping)} state constants from {INFO_H}")
    rewrite_file(mapping)
