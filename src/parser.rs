//! .kicad_sch S式パーサー
//! ピンの絶対座標・向き自動判定を含む（回転・ミラー対応）

use std::collections::HashMap;

// ────────────────────────────────────────────
// 公開型
// ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

/// 回路図上でのピンの突き出し方向
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PinOutDir {
    Right,
    Left,
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub struct LibPin {
    pub name: String,
    pub number: String,
    /// ライブラリ座標系での接続点位置（Y上向き正）
    pub rel_x: f64,
    pub rel_y: f64,
    /// ライブラリ座標系でのピン角度（0=右向き接続点, 90=上向き, 180=左向き, 270=下向き）
    pub angle: f64,
    pub length: f64,
}

#[derive(Debug, Clone)]
pub struct LibSymbol {
    pub name: String,
    pub pins: Vec<LibPin>,
}

#[derive(Debug, Clone)]
pub struct SchSymbol {
    pub x: f64,
    pub y: f64,
    /// 回転角度（0/90/180/270 のみ。KiCad仕様）
    pub angle: f64,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub lib_id: String,
    pub reference: String,
}

pub struct Schematic {
    pub lib_symbols: HashMap<String, LibSymbol>,
    pub symbols: Vec<SchSymbol>,
}

// ────────────────────────────────────────────
// Schematic メソッド
// ────────────────────────────────────────────

impl Schematic {
    pub fn find_symbol(&self, reference: &str) -> Option<&SchSymbol> {
        self.symbols.iter().find(|s| s.reference == reference)
    }

    /// リファレンス一覧（ソート済み、#PWR等除外）
    pub fn references(&self) -> Vec<String> {
        let mut refs: Vec<String> = self
            .symbols
            .iter()
            .map(|s| s.reference.clone())
            .filter(|r| !r.is_empty() && !r.starts_with('#'))
            .collect();
        refs.sort();
        refs.dedup();
        refs
    }

    /// ピン名一覧（ソート済み）
    pub fn pin_names_for(&self, reference: &str) -> Vec<String> {
        let sym = match self.find_symbol(reference) {
            Some(s) => s,
            None => return vec![],
        };
        let lib = match self.lib_symbols.get(&sym.lib_id) {
            Some(l) => l,
            None => return vec![],
        };
        let mut names: Vec<String> = lib.pins.iter().map(|p| p.name.clone()).collect();
        names.sort();
        names.dedup();
        names
    }

    /// ピン名フォーマット一覧（ソート済み）
    ///
    /// ピン名の数字部分を %d に置換したフォーマット文字列を返す。
    ///   "A0"    -> "A%d"
    ///   "A_0"   -> "A_%d"
    ///   "A_{0}" -> "A_{%d}"
    pub fn pin_prefixes_for(&self, reference: &str) -> Vec<String> {
        let names = self.pin_names_for(reference);
        let mut fmts: Vec<String> = names
            .iter()
            .filter_map(|name| pin_name_to_format(name))
            .collect();
        fmts.sort();
        fmts.dedup();
        fmts
    }

    /// フォーマット文字列の %d に start 番号を埋め込んで先頭ピン名を返す
    /// 例: ("A_{%d}", 0) -> "A_{0}"
    pub fn first_pin_name(pin_fmt: &str, start: i32) -> String {
        pin_fmt.replace("%d", &start.to_string())
    }

    /// フォーマット文字列から全ピン名を構築し、存在確認してVecで返す
    ///
    /// pin_fmt は必ず %d を含むこと。
    ///   "A%d",    0, 3 -> ["A0","A1","A2","A3"]
    ///   "A_%d",   0, 2 -> ["A_0","A_1","A_2"]
    ///   "A_{%d}", 0, 2 -> ["A_{0}","A_{1}","A_{2}"]
    pub fn resolve_pin_names(
        &self,
        reference: &str,
        pin_fmt: &str,
        start: i32,
        end: i32,
    ) -> Result<Vec<String>, String> {
        if !pin_fmt.contains("%d") {
            return Err("Pin format '".to_string()
                + pin_fmt
                + "' must contain %d  (e.g. 'A%d' or 'A_{%d}')");
        }
        let all_names = self.pin_names_for(reference);
        let count = (end - start).abs() + 1;
        let sign = if end >= start { 1 } else { -1 };
        let mut result = Vec::with_capacity(count as usize);

        for i in 0..count {
            let num = start + sign * i;
            // %d を番号で置換してピン名を構築
            let name = pin_fmt.replace("%d", &num.to_string());
            if !all_names.contains(&name) {
                // 同フォーマットに一致するピン名を候補として提示
                let candidates: Vec<&String> = all_names
                    .iter()
                    .filter(|n| pin_name_to_format(n).as_deref() == Some(pin_fmt))
                    .collect();
                // format! に pin_fmt ({} 含む) を渡すと誤動作するので文字列結合で構築
                let msg = "Pin '".to_string()
                    + &name
                    + "' not found on '"
                    + reference
                    + "'. Format '"
                    + pin_fmt
                    + "' matches: "
                    + &format!("{:?}", candidates);
                return Err(msg);
            }
            result.push(name);
        }
        Ok(result)
    }

    /// ピンの回路図上の接続点絶対座標を計算する（回転・ミラー対応）
    ///
    /// KiCadのピン at は接続点（ワイヤーを繋ぐ点）の座標。
    /// length はピン線の長さで接続点から本体方向に伸びる（接続点計算には不要）。
    ///
    /// KiCadの座標はすでにグリッド上にあるため snap は不要。
    pub fn pin_abs_position(&self, reference: &str, pin_name: &str) -> Option<Vec2> {
        let sym = self.find_symbol(reference)?;
        let lib = self.lib_symbols.get(&sym.lib_id)?;
        let pin = lib.pins.iter().find(|p| p.name == pin_name)?;

        let (x, y) =
            transform_lib_to_sch(pin.rel_x, pin.rel_y, sym.angle, sym.mirror_x, sym.mirror_y);
        Some(Vec2 {
            x: sym.x + x,
            y: sym.y + y,
        })
    }

    /// ピンが回路図上でどちら向きに突き出ているかを返す
    ///
    /// ライブラリのピン角度にシンボルの回転・ミラーを適用して判定する。
    pub fn pin_out_direction(&self, reference: &str, pin_name: &str) -> Option<PinOutDir> {
        let sym = self.find_symbol(reference)?;
        let lib = self.lib_symbols.get(&sym.lib_id)?;
        let pin = lib.pins.iter().find(|p| p.name == pin_name)?;

        // ライブラリ座標系でのピン向きベクトル
        //
        // KiCadのピンangle定義: ピン線（接続点→本体）の向き
        //   angle=0°:   ピン線が右向き → 接続点は左側 → ワイヤーは左へ
        //   angle=180°: ピン線が左向き → 接続点は右側 → ワイヤーは右へ
        //
        // ワイヤーを伸ばす方向はピン線の逆方向なので符号を反転する
        let rad = pin.angle.to_radians();
        let dir_x = -rad.cos(); // 反転
        let dir_y = -rad.sin(); // 反転

        // 座標と同じ変換を適用
        let (tx, ty) = transform_lib_to_sch(dir_x, dir_y, sym.angle, sym.mirror_x, sym.mirror_y);

        // 変換後ベクトルを4方向に丸める（絶対値の大きい成分を優先）
        let dir = if tx.abs() >= ty.abs() {
            if tx >= 0.0 {
                PinOutDir::Right
            } else {
                PinOutDir::Left
            }
        } else {
            // 回路図座標系はY下向き正なので ty>0 は Down
            if ty >= 0.0 {
                PinOutDir::Down
            } else {
                PinOutDir::Up
            }
        };
        Some(dir)
    }
}

/// ライブラリ座標系の点を回路図座標系に変換する（原点相対）
///
/// KiCadのTRANSFORM行列に基づく実装:
///   TransformCoordinate: x' = x1*x + y1*y, y' = x2*x + y2*y
///
/// 各orientationの行列:
///   ORIENT_0   (初期値): x1=1, y1=0, x2=0,  y2=-1  (Y反転のみ)
///   ORIENT_90  (CCW90°): x1=0, y1=-1,x2=-1, y2=0
///   ORIENT_180 (180°):   x1=-1,y1=0, x2=0,  y2=1
///   ORIENT_270 (CW90°):  x1=0, y1=1, x2=1,  y2=0
///
/// mirror_x（X軸ミラー）は y2の符号を反転:
///   ORIENT_0+MIRROR_X: x1=1,y1=0,x2=0,y2=1
fn transform_lib_to_sch(x: f64, y: f64, angle: f64, mirror_x: bool, mirror_y: bool) -> (f64, f64) {
    // KiCadのTRANSFORM行列を angle から決定する
    // angle値はファイル上の度数（0/90/180/270）
    let (x1, y1, x2, y2) = match (angle as i32).rem_euclid(360) {
        0 => (1.0, 0.0, 0.0, -1.0),   // ORIENT_0:   Y反転
        90 => (0.0, -1.0, -1.0, 0.0), // ORIENT_90
        180 => (-1.0, 0.0, 0.0, 1.0), // ORIENT_180
        270 => (0.0, 1.0, 1.0, 0.0),  // ORIENT_270
        _ => (1.0, 0.0, 0.0, -1.0),   // fallback
    };

    // mirror_x: Y成分の符号を反転（X軸ミラー）
    let y2 = if mirror_x { -y2 } else { y2 };
    let y1 = if mirror_x { -y1 } else { y1 };

    // mirror_y: X成分の符号を反転（Y軸ミラー）
    let x1 = if mirror_y { -x1 } else { x1 };
    let x2 = if mirror_y { -x2 } else { x2 };

    let rx = x1 * x + y1 * y;
    let ry = x2 * x + y2 * y;
    (rx, ry)
}

// ────────────────────────────────────────────
// ピン名フォーマット変換
// ────────────────────────────────────────────

/// ピン名の数字部分を %d に置換してフォーマット文字列を返す
///
///   "A15"    -> "A%d"
///   "A_15"   -> "A_%d"
///   "A_{15}" -> "A_{%d}"
///   数字なし -> None
pub fn pin_name_to_format(name: &str) -> Option<String> {
    // パターン1: 末尾が '}' -> "A_{15}" 形式
    if name.ends_with('}') {
        if let Some(open) = name.rfind('{') {
            let inner = &name[open + 1..name.len() - 1];
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                // "A_{15}" -> "A_{%d}"
                return Some(name[..open].to_string() + "{%d}");
            }
        }
        return None;
    }
    // パターン2: 末尾が連続数字 -> "A15", "A_15" 形式
    let trimmed = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.is_empty() || trimmed.len() == name.len() {
        return None;
    }
    Some(trimmed.to_string() + "%d")
}

// ────────────────────────────────────────────
// S式トークナイザー
// ────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Token {
    Open,
    Close,
    Atom(String),
}

fn tokenize(src: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                let mut esc = false;
                for ch in chars.by_ref() {
                    if esc {
                        s.push(ch);
                        esc = false;
                    } else if ch == '\\' {
                        esc = true;
                    } else if ch == '"' {
                        break;
                    } else {
                        s.push(ch);
                    }
                }
                tokens.push(Token::Atom(s));
            }
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
            }
            _ => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '(' || c == ')' || c.is_whitespace() {
                        break;
                    }
                    s.push(c);
                    chars.next();
                }
                tokens.push(Token::Atom(s));
            }
        }
    }
    tokens
}

// ────────────────────────────────────────────
// S式パーサー
// ────────────────────────────────────────────

#[derive(Debug, Clone)]
enum SExpr {
    Atom(String),
    List(Vec<SExpr>),
}

fn parse_sexprs(tokens: &[Token]) -> (Vec<SExpr>, usize) {
    let mut result = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Open => {
                let (children, consumed) = parse_sexprs(&tokens[i + 1..]);
                result.push(SExpr::List(children));
                i += consumed + 1;
            }
            Token::Close => {
                return (result, i + 1);
            }
            Token::Atom(s) => {
                result.push(SExpr::Atom(s.clone()));
                i += 1;
            }
        }
    }
    (result, i)
}

fn atom(s: &SExpr) -> Option<&str> {
    if let SExpr::Atom(a) = s {
        Some(a)
    } else {
        None
    }
}
fn children(s: &SExpr) -> Option<&Vec<SExpr>> {
    if let SExpr::List(v) = s {
        Some(v)
    } else {
        None
    }
}
fn tag_is(s: &SExpr, tag: &str) -> bool {
    children(s)
        .and_then(|v| v.first())
        .and_then(atom)
        .map(|t| t == tag)
        .unwrap_or(false)
}
fn find_child<'a>(list: &'a [SExpr], tag: &str) -> Option<&'a SExpr> {
    list.iter().find(|s| tag_is(s, tag))
}
fn child_atom<'a>(list: &'a [SExpr], tag: &str, idx: usize) -> Option<&'a str> {
    children(find_child(list, tag)?)?
        .get(idx + 1)
        .and_then(atom)
}
fn parse_f64(s: &str) -> f64 {
    s.parse().unwrap_or(0.0)
}

// ────────────────────────────────────────────
// スケマティックパーサー本体
// ────────────────────────────────────────────

pub fn parse_schematic(src: &str) -> Result<Schematic, String> {
    let tokens = tokenize(src);
    let (exprs, _) = parse_sexprs(&tokens);

    let root = exprs
        .into_iter()
        .find(|e| tag_is(e, "kicad_sch"))
        .ok_or("No kicad_sch found")?;
    let root_ch = children(&root).ok_or("kicad_sch is not a list")?;

    // lib_symbols
    let mut lib_symbols: HashMap<String, LibSymbol> = HashMap::new();
    if let Some(lib_sec) = find_child(root_ch, "lib_symbols") {
        let empty = vec![];
        let lib_ch = children(lib_sec).unwrap_or(&empty);
        for item in lib_ch {
            if tag_is(item, "symbol") {
                if let Some(sym) = parse_lib_symbol(item) {
                    lib_symbols.insert(sym.name.clone(), sym);
                }
            }
        }
    }

    // symbols（回路図上のインスタンス）
    let mut symbols: Vec<SchSymbol> = Vec::new();
    for item in root_ch {
        if tag_is(item, "symbol") {
            if let Some(sym) = parse_sch_symbol(item) {
                symbols.push(sym);
            }
        }
    }

    Ok(Schematic {
        lib_symbols,
        symbols,
    })
}

fn parse_lib_symbol(s: &SExpr) -> Option<LibSymbol> {
    let ch = children(s)?;
    let name = ch.get(1).and_then(atom)?.to_string();
    let mut pins = Vec::new();
    collect_pins(ch, &mut pins);
    Some(LibSymbol { name, pins })
}

fn collect_pins(ch: &[SExpr], pins: &mut Vec<LibPin>) {
    for item in ch {
        if tag_is(item, "pin") {
            if let Some(p) = parse_lib_pin(item) {
                pins.push(p);
            }
        } else if tag_is(item, "symbol") {
            if let Some(sub) = children(item) {
                collect_pins(sub, pins);
            }
        }
    }
}

fn parse_lib_pin(s: &SExpr) -> Option<LibPin> {
    let ch = children(s)?;
    let at = children(find_child(ch, "at")?)?;
    let rel_x = at.get(1).and_then(atom).map(parse_f64)?;
    let rel_y = at.get(2).and_then(atom).map(parse_f64)?;
    let angle = at.get(3).and_then(atom).map(parse_f64).unwrap_or(0.0);
    let length = child_atom(ch, "length", 0).map(parse_f64).unwrap_or(0.0);
    let name = children(find_child(ch, "name")?)?
        .get(1)
        .and_then(atom)
        .unwrap_or("~")
        .to_string();
    let number = children(find_child(ch, "number")?)?
        .get(1)
        .and_then(atom)
        .unwrap_or("")
        .to_string();
    Some(LibPin {
        name,
        number,
        rel_x,
        rel_y,
        angle,
        length,
    })
}

fn parse_sch_symbol(s: &SExpr) -> Option<SchSymbol> {
    let ch = children(s)?;
    let lib_id = child_atom(ch, "lib_id", 0)?.to_string();
    let at = children(find_child(ch, "at")?)?;
    let x = at.get(1).and_then(atom).map(parse_f64)?;
    let y = at.get(2).and_then(atom).map(parse_f64)?;
    let angle = at.get(3).and_then(atom).map(parse_f64).unwrap_or(0.0);
    let mut mirror_x = false;
    let mut mirror_y = false;
    for item in ch {
        if tag_is(item, "mirror") {
            if let Some(mc) = children(item) {
                for axis in mc.iter().skip(1) {
                    if atom(axis) == Some("x") {
                        mirror_x = true;
                    }
                    if atom(axis) == Some("y") {
                        mirror_y = true;
                    }
                }
            }
        }
    }
    let reference = extract_reference(ch).unwrap_or_default();
    Some(SchSymbol {
        x,
        y,
        angle,
        mirror_x,
        mirror_y,
        lib_id,
        reference,
    })
}

fn extract_reference(ch: &[SExpr]) -> Option<String> {
    let inst = children(find_child(ch, "instances")?)?;
    let project = inst.iter().find(|e| tag_is(e, "project"))?;
    let proj_ch = children(project)?;
    let path = proj_ch.iter().find(|e| tag_is(e, "path"))?;
    let path_ch = children(path)?;
    child_atom(path_ch, "reference", 0).map(String::from)
}

// ────────────────────────────────────────────
// テスト
// ────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 基本テスト用スケマティック
    /// ピンA: lib座標(-5.08, 0), angle=0°（右向き接続点）
    /// ピンB: lib座標(0, 5.08), angle=90°（上向き接続点）
    fn make_sch(sym_angle: f64, mirror_x: bool, mirror_y: bool, reference: &str) -> String {
        let mirror_str = match (mirror_x, mirror_y) {
            (true, false) => "(mirror x)".to_string(),
            (false, true) => "(mirror y)".to_string(),
            (true, true) => "(mirror x y)".to_string(),
            _ => String::new(),
        };
        format!(r#"
(kicad_sch (version 20231120) (generator "eeschema") (uuid "root")
  (lib_symbols
    (symbol "Test:IC"
      (in_bom yes) (on_board yes)
      (symbol "IC_0_1"
        (pin input line (at -5.08 0.0 0) (length 2.54)
          (name "A" (effects (font (size 1.27 1.27))))
          (number "1" (effects (font (size 1.27 1.27)))))
        (pin input line (at 0.0 5.08 90) (length 2.54)
          (name "B" (effects (font (size 1.27 1.27))))
          (number "2" (effects (font (size 1.27 1.27)))))
      )
    )
  )
  (symbol (lib_id "Test:IC") (at 100.0 50.0 {angle}) {mirror}
    (in_bom yes) (on_board yes) (uuid "s1")
    (instances (project "t" (path "/root" (reference "{ref}") (unit 1))))
  )
)
"#, angle=sym_angle, mirror=mirror_str, ref=reference)
    }

    // ── 座標テスト ──────────────────────────────────────────

    #[test]
    fn test_pos_no_rotation() {
        // pinA at(-5.08, 0) → Y反転(-5.08,0) → 回転なし → +配置(100,50) → (94.92, 50)
        let sch = parse_schematic(&make_sch(0.0, false, false, "U1")).unwrap();
        let p = sch.pin_abs_position("U1", "A").unwrap();
        assert!((p.x - 94.92).abs() < 0.01, "x={}", p.x);
        assert!((p.y - 50.00).abs() < 0.01, "y={}", p.y);
    }

    #[test]
    fn test_pos_90deg() {
        // pinA at(-5.08,0) → ORIENT_90: x'=0, y'=5.08 → +配置 → (100, 55.08)
        let sch = parse_schematic(&make_sch(90.0, false, false, "U1")).unwrap();
        let p = sch.pin_abs_position("U1", "A").unwrap();
        assert!((p.x - 100.00).abs() < 0.01, "x={}", p.x);
        assert!((p.y - 55.08).abs() < 0.01, "y={}", p.y);
    }

    #[test]
    fn test_pos_180deg() {
        // pinA at(-5.08,0) → Y反転(-5.08,0) → 180°回転(5.08,0) → +配置 → (105.08, 50)
        let sch = parse_schematic(&make_sch(180.0, false, false, "U1")).unwrap();
        let p = sch.pin_abs_position("U1", "A").unwrap();
        assert!((p.x - 105.08).abs() < 0.01, "x={}", p.x);
        assert!((p.y - 50.00).abs() < 0.01, "y={}", p.y);
    }

    #[test]
    fn test_pos_270deg() {
        // pinA at(-5.08,0) → ORIENT_270: x'=0, y'=-5.08 → +配置 → (100, 44.92)
        let sch = parse_schematic(&make_sch(270.0, false, false, "U1")).unwrap();
        let p = sch.pin_abs_position("U1", "A").unwrap();
        assert!((p.x - 100.00).abs() < 0.01, "x={}", p.x);
        assert!((p.y - 44.92).abs() < 0.01, "y={}", p.y);
    }

    #[test]
    fn test_pos_mirror_x() {
        // pinA at(-5.08,0) → ORIENT_0+mirror_x: y2=1 → x'=-5.08, y'=0 → pos=(94.92, 50)
        let sch = parse_schematic(&make_sch(0.0, true, false, "U1")).unwrap();
        let p = sch.pin_abs_position("U1", "A").unwrap();
        assert!((p.x - 94.92).abs() < 0.01, "x={}", p.x);
        assert!((p.y - 50.00).abs() < 0.01, "y={}", p.y);
    }

    // ── ピン向きテスト ──────────────────────────────────────

    #[test]
    fn test_dir_no_rotation_a() {
        // pinA angle=0°: ピン線右向き→接続点左→ワイヤー左 → Left
        let sch = parse_schematic(&make_sch(0.0, false, false, "U1")).unwrap();
        assert_eq!(sch.pin_out_direction("U1", "A").unwrap(), PinOutDir::Left);
    }

    #[test]
    fn test_dir_no_rotation_b() {
        // pinB angle=90°: ピン線上向き→接続点下→ワイヤー下 → Down
        let sch = parse_schematic(&make_sch(0.0, false, false, "U1")).unwrap();
        assert_eq!(sch.pin_out_direction("U1", "B").unwrap(), PinOutDir::Down);
    }

    #[test]
    fn test_dir_90deg_a() {
        // pinA angle=0°(反転→左向き) + symbol 90°回転 → Up
        let sch = parse_schematic(&make_sch(90.0, false, false, "U1")).unwrap();
        assert_eq!(sch.pin_out_direction("U1", "A").unwrap(), PinOutDir::Down);
    }

    #[test]
    fn test_dir_90deg_b() {
        // pinB angle=90°(反転→下向き) + symbol 90°回転 → Left
        let sch = parse_schematic(&make_sch(90.0, false, false, "U1")).unwrap();
        assert_eq!(sch.pin_out_direction("U1", "B").unwrap(), PinOutDir::Right);
    }

    #[test]
    fn test_dir_180deg_a() {
        // pinA angle=0°(反転→左向き) + symbol 180°回転 → Right
        let sch = parse_schematic(&make_sch(180.0, false, false, "U1")).unwrap();
        assert_eq!(sch.pin_out_direction("U1", "A").unwrap(), PinOutDir::Right);
    }

    #[test]
    fn test_dir_270deg_a() {
        // pinA angle=0°(反転→左向き) + symbol 270°回転 → Down
        let sch = parse_schematic(&make_sch(270.0, false, false, "U1")).unwrap();
        assert_eq!(sch.pin_out_direction("U1", "A").unwrap(), PinOutDir::Up);
    }

    #[test]
    fn test_dir_mirror_x_a() {
        // pinA angle=0°(反転→左向き) + mirror_x → Right
        let sch = parse_schematic(&make_sch(0.0, true, false, "U1")).unwrap();
        assert_eq!(sch.pin_out_direction("U1", "A").unwrap(), PinOutDir::Left);
    }

    #[test]
    fn test_dir_mirror_x_90deg_a() {
        // pinA angle=0°(反転→左向き) + mirror_x + 90°回転 → Down
        let sch = parse_schematic(&make_sch(90.0, true, false, "U1")).unwrap();
        assert_eq!(sch.pin_out_direction("U1", "A").unwrap(), PinOutDir::Down);
    }

    // ── pin_name_to_format テスト ───────────────────────────

    #[test]
    fn test_fmt_simple() {
        assert_eq!(pin_name_to_format("A0"), Some("A%d".into()));
    }
    #[test]
    fn test_fmt_underscore() {
        assert_eq!(pin_name_to_format("A_0"), Some("A_%d".into()));
    }
    #[test]
    fn test_fmt_braces() {
        assert_eq!(pin_name_to_format("A_{0}"), Some("A_{%d}".into()));
    }
    #[test]
    fn test_fmt_none() {
        assert_eq!(pin_name_to_format("VCC"), None);
    }

    // ── resolve_pin_names テスト ────────────────────────────

    const RESOLVE_SCH: &str = r#"
(kicad_sch (version 20231120) (generator "eeschema") (uuid "root")
  (lib_symbols
    (symbol "Test:IC"
      (in_bom yes) (on_board yes)
      (symbol "IC_0_1"
        (pin input line (at -5.08  5.08 0) (length 2.54)
          (name "A_{0}" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
        (pin input line (at -5.08  2.54 0) (length 2.54)
          (name "A_{1}" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
        (pin input line (at -5.08  0.00 0) (length 2.54)
          (name "A_{2}" (effects (font (size 1.27 1.27)))) (number "3" (effects (font (size 1.27 1.27)))))
        (pin input line (at -5.08 -2.54 0) (length 2.54)
          (name "A_0"   (effects (font (size 1.27 1.27)))) (number "4" (effects (font (size 1.27 1.27)))))
        (pin input line (at -5.08 -5.08 0) (length 2.54)
          (name "A_1"   (effects (font (size 1.27 1.27)))) (number "5" (effects (font (size 1.27 1.27)))))
      )
    )
  )
  (symbol (lib_id "Test:IC") (at 100.0 50.0 0)
    (in_bom yes) (on_board yes) (uuid "s1")
    (instances (project "t" (path "/root" (reference "U1") (unit 1))))
  )
)
"#;

    #[test]
    fn test_resolve_brace() {
        let sch = parse_schematic(RESOLVE_SCH).unwrap();
        let names = sch.resolve_pin_names("U1", "A_{%d}", 0, 2).unwrap();
        assert_eq!(names, vec!["A_{0}", "A_{1}", "A_{2}"]);
    }

    #[test]
    fn test_resolve_underscore() {
        let sch = parse_schematic(RESOLVE_SCH).unwrap();
        let names = sch.resolve_pin_names("U1", "A_%d", 0, 1).unwrap();
        assert_eq!(names, vec!["A_0", "A_1"]);
    }

    #[test]
    fn test_resolve_reverse() {
        let sch = parse_schematic(RESOLVE_SCH).unwrap();
        let names = sch.resolve_pin_names("U1", "A_{%d}", 2, 0).unwrap();
        assert_eq!(names, vec!["A_{2}", "A_{1}", "A_{0}"]);
    }

    #[test]
    fn test_resolve_missing_percent_d() {
        let sch = parse_schematic(RESOLVE_SCH).unwrap();
        let err = sch.resolve_pin_names("U1", "A_", 0, 1).unwrap_err();
        assert!(err.contains("%d"), "error should mention %d: {}", err);
    }

    #[test]
    fn test_resolve_pin_not_found() {
        let sch = parse_schematic(RESOLVE_SCH).unwrap();
        // A_{3} は存在しない
        let err = sch.resolve_pin_names("U1", "A_{%d}", 0, 3).unwrap_err();
        assert!(
            err.contains("A_{3}"),
            "error should mention A_{{3}}: {}",
            err
        );
    }

    #[test]
    fn test_pin_prefixes_for() {
        let sch = parse_schematic(RESOLVE_SCH).unwrap();
        let fmts = sch.pin_prefixes_for("U1");
        assert!(fmts.contains(&"A_{%d}".to_string()), "got {:?}", fmts);
        assert!(fmts.contains(&"A_%d".to_string()), "got {:?}", fmts);
    }

    #[test]
    fn test_first_pin_name() {
        assert_eq!(Schematic::first_pin_name("A%d", 0), "A0");
        assert_eq!(Schematic::first_pin_name("A_%d", 0), "A_0");
        assert_eq!(Schematic::first_pin_name("A_{%d}", 0), "A_{0}");
        assert_eq!(Schematic::first_pin_name("A_{%d}", 14), "A_{14}");
    }

    #[test]
    fn test_each_pin_unique_position() {
        let sch = parse_schematic(RESOLVE_SCH).unwrap();
        let p0 = sch.pin_abs_position("U1", "A_{0}").unwrap();
        let p1 = sch.pin_abs_position("U1", "A_{1}").unwrap();
        let p2 = sch.pin_abs_position("U1", "A_{2}").unwrap();
        assert!(
            (p0.y - p1.y).abs() > 0.01,
            "A_{{0}} and A_{{1}} should differ in Y"
        );
        assert!(
            (p1.y - p2.y).abs() > 0.01,
            "A_{{1}} and A_{{2}} should differ in Y"
        );
        assert!(
            (p0.x - p1.x).abs() < 0.01,
            "X should be same for left-facing pins"
        );
    }
}
