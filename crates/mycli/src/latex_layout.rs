//! Small terminal math layouts. Unsupported structures keep their source.
use unicode_width::UnicodeWidthStr;

const MAX_BYTES: usize = 4000;
const MAX_DEPTH: usize = 16;

pub(crate) struct Environment<'a> {
    pub name: &'a str,
    pub body: &'a str,
    pub end: usize,
}

/// Match nested environments, requiring each closing name to match its opener.
pub(crate) fn environment_at(text: &str, start: usize) -> Option<Environment<'_>> {
    let rest = text.get(start..)?.strip_prefix(r"\begin{")?;
    let name_len = rest.find('}')?;
    let name = &rest[..name_len];
    let body_start = start + 7 + name_len + 1;
    let mut stack = vec![name];
    let mut i = body_start;
    while i < text.len() {
        if text[i..].starts_with(r"\begin{") || text[i..].starts_with(r"\end{") {
            let opening = text[i..].starts_with(r"\begin{");
            let prefix = if opening { 7 } else { 5 };
            let stop = i + prefix + text[i + prefix..].find('}')?;
            let nested = &text[i + prefix..stop];
            if opening {
                if stack.len() >= MAX_DEPTH { return None; }
                stack.push(nested);
            } else {
                if stack.pop()? != nested { return None; }
                if stack.is_empty() {
                    return Some(Environment { name, body: &text[body_start..i], end: stop + 1 });
                }
            }
            i = stop + 1;
        } else {
            let c = text[i..].chars().next()?;
            i += c.len_utf8();
            if c == '\\' {
                // Escaped punctuation, including \\, cannot open an environment.
                if let Some(next) = text[i..].chars().next() { i += next.len_utf8(); }
            }
        }
    }
    None
}

pub(crate) fn supported(name: &str) -> bool {
    matches!(name, "matrix" | "pmatrix" | "bmatrix" | "Bmatrix" | "vmatrix" | "Vmatrix"
        | "aligned" | "align" | "align*" | "cases" | "equation" | "equation*")
}

#[derive(Clone)]
struct Block { lines: Vec<String> }
impl Block {
    fn text(text: String) -> Self {
        let lines = text.split('\n').map(str::to_string).collect();
        Self { lines }
    }
    fn width(&self) -> usize { self.lines.iter().map(|s| s.width()).max().unwrap_or(0) }
    fn baseline(&self) -> usize { self.lines.len().saturating_sub(1) / 2 }
    fn finish(&self) -> String { self.lines.iter().map(|s| s.trim_end()).collect::<Vec<_>>().join("\n") }
}

/// Join mathematical objects at their vertical centers rather than appending
/// an operator to the bottom row of the preceding matrix.
fn join(blocks: &[Block]) -> Block {
    let baseline = blocks.iter().map(Block::baseline).max().unwrap_or(0);
    let below = blocks.iter().map(|b| b.lines.len() - b.baseline() - 1).max().unwrap_or(0);
    let mut lines = vec![String::new(); baseline + below + 1];
    for block in blocks {
        let top = baseline - block.baseline();
        let width = block.width();
        for (row, line) in lines.iter_mut().enumerate() {
            let text = row.checked_sub(top).and_then(|i| block.lines.get(i)).map(String::as_str).unwrap_or("");
            line.push_str(text);
            line.push_str(&" ".repeat(width.saturating_sub(text.width())));
        }
    }
    Block { lines }
}

pub(crate) fn render(expr: &str) -> String {
    if expr.len() > MAX_BYTES { return expr.into(); }
    render_inner(expr, 0).map(|b| b.finish()).unwrap_or_else(|| expr.into())
}

fn render_inner(expr: &str, depth: usize) -> Option<Block> {
    if depth > MAX_DEPTH { return None; }
    let mut blocks = Vec::new();
    let mut from = 0;
    while let Some(relative) = expr[from..].find(r"\begin{") {
        let start = from + relative;
        // Environments inside a command argument need a full TeX parser.
        // Preserve the entire expression instead of producing a partial layout.
        let prefix = &expr[from..start];
        if prefix.chars().filter(|c| *c == '{').count() != prefix.chars().filter(|c| *c == '}').count() {
            return None;
        }
        let env = environment_at(expr, start)?;
        if !supported(env.name) { return None; }
        if !prefix.is_empty() {
            let text = super::latex::convert(&prefix.replace('\n', " "));
            let text = if matches!(text.trim(), "=" | "+" | "-" | "≤" | "≥" | "≠" | "×" | "·") {
                format!(" {} ", text.trim())
            } else { text };
            blocks.push(Block::text(text));
        } else if !blocks.is_empty() {
            blocks.push(Block::text(" ".into()));
        }
        let block = if matches!(env.name, "equation" | "equation*") {
            render_inner(env.body.trim(), depth + 1)?
        } else {
            grid(env.name, env.body, depth + 1)?
        };
        blocks.push(block);
        from = env.end;
    }
    let rest = &expr[from..];
    if rest.contains(r"\end{") { return None; }
    if !rest.is_empty() || blocks.is_empty() {
        let rest = if blocks.is_empty() { rest.to_string() } else { rest.replace('\n', " ") };
        blocks.push(Block::text(super::latex::convert(&rest)));
    }
    Some(join(&blocks))
}

/// Split only top-level separators. Nested matrices and escaped ampersands
/// belong to their cells and must not change the surrounding grid dimensions.
fn cells(body: &str) -> Option<Vec<Vec<String>>> {
    let mut rows = vec![vec![String::new()]];
    let mut braces = 0usize;
    let mut i = 0;
    while i < body.len() {
        let remaining = &body[i..];
        if remaining.starts_with(r"\begin{") {
            let env = environment_at(body, i)?;
            rows.last_mut()?.last_mut()?.push_str(&body[i..env.end]);
            i = env.end;
            continue;
        }
        if remaining.starts_with(r"\\") && braces == 0 {
            rows.push(vec![String::new()]);
            if rows.len() > 64 { return None; }
            i += 2;
            // TeX row-spacing annotations affect presentation, not cell values.
            if body[i..].starts_with('[') { i += body[i..].find(']')? + 1; }
            continue;
        }
        let c = remaining.chars().next()?;
        i += c.len_utf8();
        if c == '&' && braces == 0 {
            rows.last_mut()?.push(String::new());
            if rows.last()?.len() > 16 { return None; }
            continue;
        }
        if c == '{' { braces += 1; }
        if c == '}' { braces = braces.checked_sub(1)?; }
        rows.last_mut()?.last_mut()?.push(c);
        if c == '\\' {
            if let Some(next) = body[i..].chars().next() {
                rows.last_mut()?.last_mut()?.push(next);
                i += next.len_utf8();
            }
        }
    }
    if braces != 0 { return None; }
    if rows.last().is_some_and(|row| row.len() == 1 && row[0].trim().is_empty()) { rows.pop(); }
    if rows.is_empty() { return None; }
    Some(rows)
}

fn grid(name: &str, body: &str, depth: usize) -> Option<Block> {
    let cells = cells(body)?;
    let columns = cells.iter().map(Vec::len).max()?;
    let aligned = matches!(name, "aligned" | "align" | "align*");
    if !aligned && cells.iter().any(|r| r.len() != columns) { return None; }
    if name == "cases" && columns > 2 { return None; }
    let mut rows = Vec::new();
    let mut widths = vec![0; columns];
    for row in cells {
        let mut blocks = Vec::new();
        for (c, text) in row.iter().enumerate() {
            let block = render_inner(text.trim(), depth + 1)?;
            widths[c] = widths[c].max(block.width());
            blocks.push(block);
        }
        rows.push(blocks);
    }
    let mut lines = Vec::new();
    for row in rows {
        let mut padded = Vec::new();
        for (c, width) in widths.iter().enumerate() {
            let block = row.get(c).cloned().unwrap_or_else(|| Block::text(String::new()));
            let mut block = Block { lines: block.lines.iter().map(|line| {
                let spaces = " ".repeat(width.saturating_sub(line.width()));
                if aligned && c % 2 == 0 { format!("{spaces}{line}") } else { format!("{line}{spaces}") }
            }).collect() };
            if c + 1 < columns {
                let gap = if aligned && c % 2 == 0 { " " } else { "  " };
                for line in &mut block.lines { line.push_str(gap); }
            }
            padded.push(block);
        }
        lines.extend(join(&padded).lines);
    }
    let height = lines.len();
    for (i, line) in lines.iter_mut().enumerate() {
        let first = i == 0;
        let last = i + 1 == height;
        let (left, right) = match name {
            "bmatrix" if height == 1 => ("[", "]"),
            "bmatrix" => (if first { "⎡" } else if last { "⎣" } else { "⎢" }, if first { "⎤" } else if last { "⎦" } else { "⎥" }),
            "pmatrix" if height == 1 => ("(", ")"),
            "pmatrix" => (if first { "⎛" } else if last { "⎝" } else { "⎜" }, if first { "⎞" } else if last { "⎠" } else { "⎟" }),
            "Bmatrix" | "cases" => (
                if height == 1 { "{" } else if first { "⎧" } else if last { "⎩" } else if i == height / 2 { "⎨" } else { "⎪" },
                if name == "cases" { "" } else if height == 1 { "}" } else if first { "⎫" } else if last { "⎭" } else if i == height / 2 { "⎬" } else { "⎪" }),
            "vmatrix" => ("│", "│"), "Vmatrix" => ("‖", "‖"), _ => ("", ""),
        };
        if !left.is_empty() { *line = format!("{left} {line}{}", if right.is_empty() { String::new() } else { format!(" {right}") }); }
    }
    Some(Block { lines })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matrix_products_share_a_baseline() {
        let source = r"\begin{bmatrix}a&b\\c&d\end{bmatrix}\begin{bmatrix}x\\y\end{bmatrix}=\begin{bmatrix}ax+by\\cx+dy\end{bmatrix}";
        let result = render(source);
        assert_eq!(result, "⎡ a  b ⎤ ⎡ x ⎤ = ⎡ ax+by ⎤\n⎣ c  d ⎦ ⎣ y ⎦   ⎣ cx+dy ⎦");
    }
    #[test]
    fn aligns_equations_and_piecewise_conditions() {
        let result = render(r"\begin{aligned}x&=1\\long&=2\end{aligned}");
        assert_eq!(result, "   x =1\nlong =2");
        let cases = render(r"f(x)=\begin{cases}x^2&x>0\\0&x\leq0\end{cases}");
        assert!(cases.contains("⎧ x²  x>0") && cases.contains("⎩ 0   x≤0"), "{cases}");
    }
    #[test]
    fn unicode_cells_and_nested_matrices_keep_alignment() {
        let result = render(r"\begin{bmatrix}\text{漢}&x\\a&y\end{bmatrix}");
        assert_eq!(result, "⎡ 漢  x ⎤\n⎣ a   y ⎦");
        let nested = render(r"\begin{bmatrix}\begin{pmatrix}1\\2\end{pmatrix}&3\end{bmatrix}");
        assert!(nested.contains("⎛ 1 ⎞") && nested.contains("⎝ 2 ⎠"), "{nested}");
    }
    #[test]
    fn unsupported_or_malformed_environments_are_not_destroyed() {
        for source in [r"\begin{unknown}x^2\end{unknown}", r"\begin{bmatrix}a&b\\c\end{bmatrix}", r"\begin{bmatrix}x\end{pmatrix}", r"\begin{bmatrix}x", r"\frac{\begin{matrix}x\end{matrix}}{2}"] {
            assert_eq!(render(source), source);
        }
    }
    #[test]
    fn escaped_ampersands_stay_in_their_cells() {
        let result = render(r"\begin{bmatrix}\text{a\&b}&c\\d&e\end{bmatrix}");
        assert!(result.contains("a&b"), "{result}");
    }
}
