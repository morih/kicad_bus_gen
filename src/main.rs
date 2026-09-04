mod parser;
mod generator;
mod tui;
mod session;

use std::env;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 || args[1] == "--help" {
        print_help();
        return;
    }

    // デバッグ: kicad_bus_gen --list-pins U1 file.kicad_sch
    if args.len() == 4 && args[1] == "--list-pins" {
        let reference = &args[2];
        let path = std::path::PathBuf::from(&args[3]);
        list_pins(&path, reference);
        return;
    }
    // デバッグ: kicad_bus_gen --pin-detail U2 A0 file.kicad_sch  (* で全ピン)
    if args.len() == 5 && args[1] == "--pin-detail" {
        let reference = &args[2];
        let pin_name  = &args[3];
        let path = std::path::PathBuf::from(&args[4]);
        list_pin_detail(&path, reference, pin_name);
        return;
    }

    let sch_path = PathBuf::from(&args[1]);
    if !sch_path.exists() {
        eprintln!("Error: file not found: {}", sch_path.display());
        std::process::exit(1);
    }

    let schematic_src = std::fs::read_to_string(&sch_path)
        .expect("Failed to read schematic file");

    let schematic = match parser::parse_schematic(&schematic_src) {
        Ok(s) => s,
        Err(e) => { eprintln!("Error parsing schematic: {}", e); std::process::exit(1); }
    };

    // 前回セッションを復元
    let saved = session::load(&sch_path);

    let bus_specs = match tui::run_tui(&schematic, saved) {
        Ok(specs) => specs,
        Err(e) => { eprintln!("TUI error: {}", e); std::process::exit(1); }
    };

    if bus_specs.is_empty() {
        println!("No bus specs defined, exiting.");
        return;
    }

    // 生成前にセッション保存
    session::save(&sch_path, &bus_specs.iter().map(|s| session::SavedRow {
        reference:  s.reference.clone(),
        pin_prefix: s.pin_prefix.clone(),
        prefix:     s.prefix.clone(),
        start:      s.start,
        end:        s.end,
        wire_len:   s.wire_len,
    }).collect::<Vec<_>>());

    match generator::generate_and_append(&sch_path, &schematic_src, &schematic, &bus_specs) {
        Ok(count) => println!("Generated {} bus group(s) successfully.", count),
        Err(e)    => { eprintln!("Generation error: {}", e); std::process::exit(1); }
    }
}

fn print_help() {
    println!("kicad_bus_gen - KiCad Bus Generator");
    println!("Usage: kicad_bus_gen <schematic.kicad_sch>");
    println!();
    println!("Pin Prefix format examples:");
    println!("  A%d      -> A0, A1, ... A15");
    println!("  A_%d     -> A_0, A_1, ...");
    println!("  A_{{%d}} -> A_{{0}}, A_{{1}}, ...");
    println!();
    println!("TUI Keys:");
    println!("  Tab / Shift+Tab  Move between fields");
    println!("  a                Add row");
    println!("  d                Delete row");
    println!("  g                Generate and write to file");
    println!("  q / Esc          Quit without generating");
}

// デバッグコマンド: `kicad_bus_gen --list-pins <ref> <schematic>` でピン一覧を表示
// A11がない場合の調査用
fn list_pins(sch_path: &std::path::PathBuf, reference: &str) {
    let src = std::fs::read_to_string(sch_path).expect("read failed");
    let sch = parser::parse_schematic(&src).expect("parse failed");
    let names = sch.pin_names_for(reference);
    println!("Pins for {}:", reference);
    for n in &names { println!("  {:?}", n); }
    println!("Formats:");
    for f in sch.pin_prefixes_for(reference) { println!("  {:?}", f); }
}

/// デバッグ: ピンの詳細情報（座標・angle）を表示
fn list_pin_detail(sch_path: &std::path::PathBuf, reference: &str, pin_name: &str) {
    let src = std::fs::read_to_string(sch_path).expect("read failed");
    let sch = parser::parse_schematic(&src).expect("parse failed");

    let sym = match sch.find_symbol(reference) {
        Some(s) => s,
        None => { eprintln!("Symbol '{}' not found", reference); return; }
    };
    let lib = match sch.lib_symbols.get(&sym.lib_id) {
        Some(l) => l,
        None => { eprintln!("LibSymbol '{}' not found", sym.lib_id); return; }
    };

    println!("Symbol: {} at ({:.4}, {:.4}) angle={} mirror_x={} mirror_y={}",
        reference, sym.x, sym.y, sym.angle, sym.mirror_x, sym.mirror_y);

    for pin in &lib.pins {
        if pin_name == "*" || pin.name == pin_name {
            let pos = sch.pin_abs_position(reference, &pin.name);
            let dir = sch.pin_out_direction(reference, &pin.name);
            println!("  pin {:10} at=({:7.2},{:7.2}) angle={:5} len={:.2} -> pos={:?} dir={:?}",
                pin.name, pin.rel_x, pin.rel_y, pin.angle, pin.length,
                pos.as_ref().map(|p| (p.x, p.y)),
                dir);
        }
    }
}
