# Red Faction Guerrilla 1.0.2.1: reverse-engineering reference

Facts about `RFG.exe` collected while rebuilding the game's destruction, melee and animation rules in Rust, so nobody has to
repeat the searching. **No game code, decompiled source, binaries or memory dumps are included**, only addresses, names,
sizes, call edges, layouts and notes.

Target build: Steam app 20500, `RFG.exe` 1.0.2.1, 9,168,528 bytes, 32-bit PE, link timestamp 2016-12-12,
SHA256 `0606bbed95481ee6d54b5a926adaccaa29db6e2e42e2fff09fe9dfd25946c72d` (see `RFG_1.0.2.1_steam/hashes.json`).

## What is here (`RFG_1.0.2.1_steam/`)
| File | Contents |
|---|---|
| `functions.csv` | All 22,251 functions Ghidra found in the memory image: `address,size,ghidra_name,known_name,thunk,callees,prototype`. Addresses are virtual addresses (image base 0x400000). `known_name` is filled for the functions listed in `symbols.csv`. `prototype` is Ghidra's automatic decompiler signature (often only `undefinedN` parameters; empty for 567 functions whose decompilation failed). |
| `callgraph.csv` | 69,798 direct call edges `caller,callee` (virtual addresses), from Ghidra's call references. Indirect calls (vtables, function pointers) are not included. |
| `symbols.csv` | Curated names for 73 addresses: 57 functions, 16 data items, constants and code sites, with area, a one-line summary, the evidence ID in `../../Research/evidence_registry.md`, a confidence rating and whether the address lies in the memory image. Names are proposals, not the developers' names. |
| `structs.md` | Record layouts (material, melee attack, camera shake), human/subpiece offsets, the damage vtable and the constants. |
| `hashes.json` | Hashes of the exe and of the memory image, so you can check yours. |

The write-ups behind the names are `../Research/evidence_registry.md` (numbered findings) and `../Docs/destruction_reference.md`.

## Analysing the game yourself
`RFG.exe` is packed on disk (Steam DRM, a `.bind` section and an entry point outside the file's code), so static analysis of the file
finds only a handful of functions. The addresses above come from the **unpacked image in memory**:

1. Start the single-player game and let it reach the main menu.
2. With any memory reader, read the process image from RVA `0x1000` to `0x75D000` (`.text` is `0x1000-0x6C9000`, `.rdata`
   `0x6C9000-0x75D000`) and write it to a file whose offset equals the RVA (7,720,960 bytes; zero-fill the first 0x1000 bytes).
   `.data` (VA 0xB5D000 and up) is not covered; globals there are listed as "not in dump".
3. Check the file and its two sections against `hashes.json` (they should match unless your game build or reader differs).
4. In Ghidra: language `x86:LE:32:default:windows`, raw binary, base address `0x400000`, run auto-analysis. Function entry
   addresses should then match `functions.csv` (for example `0x006e2080`, the building damage virtual).

The data tables (`*.xtbl`, `misc.vpp_pc`) are plain XML after `ExtractAssets.bat` has run, so many "constants" can be read there instead of in code.

## Conventions
- Addresses are 8 hex digits, virtual addresses in the unpacked image, matching Ghidra's `FUN_xxxxxxxx` names.
- "E-0nn" refers to a row in `Research/evidence_registry.md`; confidence there is "High", "Medium" or "Low" for the finding.
- Nothing here has been validated against other builds (for example Re-Mars-tered, which is a 64-bit rebuild with different formats).
