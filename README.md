# kicad_bus_gen

A command-line tool that automatically generates bus wiring elements (bus entries, wires, and net labels) in KiCad 10 schematic files (`.kicad_sch`).

Pin coordinates and directions are calculated automatically from the schematic, and the elements are appended directly to the file. Since the KiCad Python API for schematic editing is not yet implemented, this tool edits the file directly.

[日本語版 README](README.ja.md)

## Requirements

- KiCad 10
- Rust 1.75 or later
- macOS / Linux

## Build

```bash
git clone ...
cd kicad_bus_gen
cargo build --release
```

The binary will be generated at `target/release/kicad_bus_gen`.

## Usage

```bash
./kicad_bus_gen <schematic.kicad_sch>
```

A TUI will launch. Enter the pin groups you want to connect to the bus, then press `g` to generate and write to the schematic file.

### TUI Controls

| Key | Action |
|-----|--------|
| `←` `→` | Move between columns |
| `↑` `↓` | Move between rows |
| `Tab` / `Shift+Tab` | Next / previous column |
| `Enter` | Confirm edit and move to next column |
| `a` | Add row |
| `d` | Delete row |
| `g` | Generate bus elements and write to file |
| `q` / `Esc` | Quit without generating |

### Input Columns

| Column | Description | Example |
|--------|-------------|---------|
| Ref | Symbol reference | `U1` |
| Pin Prefix | Pin name format (must contain `%d`) | `A_{%d}` `A%d` `D_%d` |
| Prefix | Net label prefix to generate | `A` `D` |
| Start | Starting number | `0` |
| End | Ending number (reverse order supported) | `15` |
| Wire(inch) | Wire length in inches | `0.2` |

### Pin Prefix Format

| Format | Matches |
|--------|---------|
| `A%d` | `A0`, `A1`, ... `A15` |
| `A_%d` | `A_0`, `A_1`, ... |
| `A_{%d}` | `A_{0}`, `A_{1}`, ... |

Pin name suggestions are shown in the Suggestions area of the TUI.

### Example

To generate bus wiring for Z80 address lines A0–A15:

```
Ref: U1    Pin Prefix: A_{%d}   Prefix: A   Start: 0   End: 15   Wire: 0.2
```

## Debug Commands

```bash
# List pin names and formats for a symbol
./kicad_bus_gen --list-pins U1 schematic.kicad_sch

# Show coordinate and direction for a specific pin
./kicad_bus_gen --pin-detail U1 "A_{0}" schematic.kicad_sch

# Show details for all pins
./kicad_bus_gen --pin-detail U1 "*" schematic.kicad_sch
```

## Generated Elements

For each pin, the following three elements are generated:

```
(wire)       from pin connection point to bus entry start
(bus_entry)  diagonal bus entry line
(label)      net label placed at the bus entry endpoint
```

The pin direction (Right / Left / Up / Down) is automatically determined from the symbol's rotation and mirror settings.

## Session Saving

Input values are automatically saved to `<schematic>.bus_gen.json` after generation. The previous session is restored on the next launch.

## Notes

- Generation **appends** to the file. Running the tool multiple times with the same settings will create duplicate elements. Use KiCad's Undo or delete manually if this happens.
- Pin coordinate transformation is implemented based on KiCad's TRANSFORM matrix specification, supporting symbol rotation (0°/90°/180°/270°) and mirroring (X-axis, Y-axis).

## File Structure

```
src/
├── main.rs       Entry point and debug commands
├── parser.rs     .kicad_sch S-expression parser, pin coordinate calculation, direction detection
├── generator.rs  Bus element generation and file writing
├── tui.rs        ratatui TUI
└── session.rs    Session persistence (JSON)
```

## Dependencies

```toml
uuid = { version = "1", features = ["v4"] }
rand = "0.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ratatui = "0.29"
crossterm = "0.28"
```

## License

MIT
