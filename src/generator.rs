//! バス要素（ネットラベル・ワイヤー・バスエントリ）を生成し .kicad_sch に追記する

use crate::parser::{PinOutDir, Schematic};
use crate::tui::BusSpec;
use std::path::Path;
use uuid::Uuid;

const BUS_ENTRY_SIZE: f64 = 2.54;
const SNAP_GRID: f64 = 1.27;

fn snap(v: f64) -> f64 {
    (v / SNAP_GRID).round() * SNAP_GRID
}
fn new_uuid() -> String {
    Uuid::new_v4().to_string()
}

// ────────────────────────────────────────────
// レイアウトパラメータ
// ────────────────────────────────────────────

/// KiCadのラベルアンカーはワイヤーの端点に置く必要がある
/// （中間点に置くと未接続マーカーが表示される）
///
/// アンカー位置の選択:
///   Right/Left: ピン接続点（wire始点）に置く → justify left/right でテキスト方向を制御
///   Up/Down:    同様
///
/// justifyの効果:
///   angle=0,   justify left  → テキストがアンカーから右へ伸びる
///   angle=0,   justify right → テキストがアンカーから左へ伸びる
///   angle=180, justify left  → テキストがアンカーから左へ伸びる（angle=180で反転）
///   angle=90,  justify left  → テキストがアンカーから上へ伸びる
#[derive(Clone)]
struct Layout {
    wire_dx: f64,
    wire_dy: f64,
    entry_sx: f64,
    entry_sy: f64,
    label_angle: f64,
    /// ラベルアンカーのオフセット（ピン接続点からの相対位置）
    label_dx: f64,
    label_dy: f64,
    label_justify: &'static str,
}

impl Layout {
    fn from(pin_dir: PinOutDir, wire_len: f64) -> Self {
        match pin_dir {
            // Right: ワイヤーが右向き
            // ラベルアンカー = ピン接続点(wire始点)
            // angle=0, justify left → テキストが右（ワイヤー方向）へ
            // ワイヤーと重なるが、KiCadの標準的な配置はこれ
            // バスエントリatに置くとバスの外側にテキストが出てしまう
            // Right: アンカー=バスエントリat（ワイヤー終端）
            // angle=180, justify right bottom → テキストがアンカーより左（ピン側）
            // バスすれすれの位置にテキストが来る
            PinOutDir::Right => Self {
                wire_dx: wire_len,
                wire_dy: 0.0,
                entry_sx: BUS_ENTRY_SIZE,
                entry_sy: BUS_ENTRY_SIZE,
                label_angle: 180.0,
                label_dx: wire_len,
                label_dy: 0.0,
                label_justify: "right bottom",
            },
            // Left: アンカー=バスエントリat（ワイヤー終端）
            // angle=0, justify left bottom → テキストがアンカーより右（ピン側）
            // バスすれすれの位置にテキストが来る
            PinOutDir::Left => Self {
                wire_dx: -wire_len,
                wire_dy: 0.0,
                entry_sx: -BUS_ENTRY_SIZE,
                entry_sy: BUS_ENTRY_SIZE,
                label_angle: 0.0,
                label_dx: -wire_len,
                label_dy: 0.0,
                label_justify: "left bottom",
            },
            // Up: アンカー=バスエントリat（ワイヤー終端）
            // angle=270, justify right bottom → テキストがアンカーより下（ピン側）
            PinOutDir::Up => Self {
                wire_dx: 0.0,
                wire_dy: -wire_len,
                entry_sx: -BUS_ENTRY_SIZE,
                entry_sy: -BUS_ENTRY_SIZE,
                label_angle: 270.0,
                label_dx: 0.0,
                label_dy: -wire_len,
                label_justify: "right bottom",
            },
            // Down: アンカー=バスエントリat（ワイヤー終端）
            // angle=90, justify left bottom → テキストがアンカーより上（ピン側）
            PinOutDir::Down => Self {
                wire_dx: 0.0,
                wire_dy: wire_len,
                entry_sx: -BUS_ENTRY_SIZE,
                entry_sy: BUS_ENTRY_SIZE,
                label_angle: 90.0,
                label_dx: 0.0,
                label_dy: wire_len,
                label_justify: "left bottom",
            },
        }
    }

    fn from_inverted(pin_dir: PinOutDir, wire_len: f64) -> Self {
        let opposite = match pin_dir {
            PinOutDir::Right => PinOutDir::Left,
            PinOutDir::Left => PinOutDir::Right,
            PinOutDir::Up => PinOutDir::Down,
            PinOutDir::Down => PinOutDir::Up,
        };
        Self::from(opposite, wire_len)
    }
}

// ────────────────────────────────────────────
// 生成・書き込み
// ────────────────────────────────────────────

pub fn generate_and_append(
    sch_path: &Path,
    src: &str,
    schematic: &Schematic,
    specs: &[BusSpec],
) -> Result<usize, String> {
    let mut generated = String::new();
    let mut count = 0;

    for spec in specs {
        let pin_names =
            schematic.resolve_pin_names(&spec.reference, &spec.pin_prefix, spec.start, spec.end)?;

        let pin_dir = schematic
            .pin_out_direction(&spec.reference, &pin_names[0])
            .ok_or_else(|| {
                "Cannot determine direction for pin '".to_string()
                    + &pin_names[0]
                    + "' on '"
                    + &spec.reference
                    + "'"
            })?;

        let layout = Layout::from(pin_dir, spec.wire_len);
        let sign = if spec.end >= spec.start { 1 } else { -1 };

        for (i, pin_name) in pin_names.iter().enumerate() {
            let num = spec.start + sign * i as i32;
            let net_label = spec.prefix.clone() + &num.to_string();

            let pos = schematic
                .pin_abs_position(&spec.reference, pin_name)
                .ok_or_else(|| {
                    "Cannot get position for pin '".to_string()
                        + pin_name
                        + "' on '"
                        + &spec.reference
                        + "'"
                })?;

            generated.push_str(&gen_pin_elements(pos.x, pos.y, &net_label, &layout));
        }
        count += 1;
    }

    let insert_pos = src.rfind(')').ok_or("Invalid .kicad_sch: no closing ')'")?;
    let new_content = format!("{}{}\n)", &src[..insert_pos], generated);
    std::fs::write(sch_path, new_content).map_err(|e| format!("Write error: {}", e))?;

    Ok(count)
}

fn gen_pin_elements(pin_x: f64, pin_y: f64, net_name: &str, layout: &Layout) -> String {
    let entry_x = snap(pin_x + layout.wire_dx);
    let entry_y = snap(pin_y + layout.wire_dy);
    let label_x = snap(pin_x + layout.label_dx);
    let label_y = snap(pin_y + layout.label_dy);

    format!(
        r#"
  (bus_entry (at {ex:.4} {ey:.4}) (size {esx:.4} {esy:.4})
    (stroke (width 0) (type default))
    (uuid "{u1}")
  )
  (wire (pts (xy {wx1:.4} {wy1:.4}) (xy {wx2:.4} {wy2:.4}))
    (stroke (width 0) (type default))
    (uuid "{u2}")
  )
  (label "{net}" (at {lx:.4} {ly:.4} {la})
    (effects (font (size 1.27 1.27)) (justify {just}))
    (uuid "{u3}")
  )"#,
        ex = entry_x,
        ey = entry_y,
        esx = layout.entry_sx,
        esy = layout.entry_sy,
        u1 = new_uuid(),
        wx1 = pin_x,
        wy1 = pin_y,
        wx2 = entry_x,
        wy2 = entry_y,
        u2 = new_uuid(),
        net = net_name,
        lx = label_x,
        ly = label_y,
        la = layout.label_angle,
        just = layout.label_justify,
        u3 = new_uuid(),
    )
}
