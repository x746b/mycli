//! Turning LaTeX math into readable terminal text.
//!
//! Cloud models answer maths questions in LaTeX — `\(x^2\)` inline, `\[ ... \]`
//! or `$$ ... $$` for display — and a terminal has no way to typeset it, so it
//! arrives as literal backslashes and braces. There is no faithful rendering
//! available; the goal is only that a reader can follow the maths, which
//! Unicode gets most of the way to: superscripts, Greek letters, and the
//! operator symbols LaTeX spells out as words.
//!
//! Anything unrecognised is left as-is rather than mangled, so an expression
//! this module cannot handle is no worse off than before.

use std::collections::HashMap;
use std::sync::OnceLock;

// ─── Symbol tables ──────────────────────────────────────────────────────────

/// LaTeX command → the character it stands for. Longest match wins, so
/// `\lambda` is not read as `\lamb` + `da`.
fn symbols() -> &'static HashMap<&'static str, &'static str> {
    static SYMBOLS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    SYMBOLS.get_or_init(|| {
        [
            // Greek, lower case
            ("alpha", "α"), ("beta", "β"), ("gamma", "γ"), ("delta", "δ"),
            ("epsilon", "ε"), ("varepsilon", "ε"), ("zeta", "ζ"), ("eta", "η"),
            ("theta", "θ"), ("vartheta", "ϑ"), ("iota", "ι"), ("kappa", "κ"),
            ("lambda", "λ"), ("mu", "μ"), ("nu", "ν"), ("xi", "ξ"),
            ("pi", "π"), ("rho", "ρ"), ("sigma", "σ"), ("tau", "τ"),
            ("upsilon", "υ"), ("phi", "φ"), ("varphi", "φ"), ("chi", "χ"),
            ("psi", "ψ"), ("omega", "ω"),
            // Greek, upper case
            ("Gamma", "Γ"), ("Delta", "Δ"), ("Theta", "Θ"), ("Lambda", "Λ"),
            ("Xi", "Ξ"), ("Pi", "Π"), ("Sigma", "Σ"), ("Upsilon", "Υ"),
            ("Phi", "Φ"), ("Psi", "Ψ"), ("Omega", "Ω"),
            // Relations
            ("leq", "≤"), ("le", "≤"), ("geq", "≥"), ("ge", "≥"),
            ("neq", "≠"), ("ne", "≠"), ("approx", "≈"), ("equiv", "≡"),
            ("sim", "∼"), ("simeq", "≃"), ("cong", "≅"), ("propto", "∝"),
            ("ll", "≪"), ("gg", "≫"),
            // Operators
            ("times", "×"), ("div", "÷"), ("cdot", "·"), ("pm", "±"),
            ("mp", "∓"), ("ast", "∗"), ("star", "⋆"), ("circ", "∘"),
            ("oplus", "⊕"), ("otimes", "⊗"),
            // Big operators
            ("sum", "∑"), ("prod", "∏"), ("int", "∫"), ("iint", "∬"),
            ("oint", "∮"), ("bigcup", "⋃"), ("bigcap", "⋂"),
            // Sets and logic
            ("in", "∈"), ("notin", "∉"), ("subset", "⊂"), ("subseteq", "⊆"),
            ("supset", "⊃"), ("supseteq", "⊇"), ("cup", "∪"), ("cap", "∩"),
            ("emptyset", "∅"), ("varnothing", "∅"), ("forall", "∀"),
            ("exists", "∃"), ("nexists", "∄"), ("neg", "¬"), ("lnot", "¬"),
            ("land", "∧"), ("wedge", "∧"), ("lor", "∨"), ("vee", "∨"),
            ("setminus", "∖"),
            // Number sets
            ("mathbb{R}", "ℝ"), ("mathbb{N}", "ℕ"), ("mathbb{Z}", "ℤ"),
            ("mathbb{Q}", "ℚ"), ("mathbb{C}", "ℂ"),
            // Arrows
            ("to", "→"), ("rightarrow", "→"), ("Rightarrow", "⇒"),
            ("leftarrow", "←"), ("Leftarrow", "⇐"), ("leftrightarrow", "↔"),
            ("Leftrightarrow", "⇔"), ("mapsto", "↦"), ("implies", "⟹"),
            ("iff", "⟺"),
            // Miscellany
            ("infty", "∞"), ("partial", "∂"), ("nabla", "∇"), ("angle", "∠"),
            ("perp", "⊥"), ("parallel", "∥"), ("therefore", "∴"),
            ("because", "∵"), ("dots", "…"), ("ldots", "…"), ("cdots", "⋯"),
            ("vdots", "⋮"), ("ddots", "⋱"), ("prime", "′"), ("degree", "°"),
            ("aleph", "ℵ"), ("hbar", "ℏ"), ("ell", "ℓ"), ("Re", "ℜ"), ("Im", "ℑ"),
            ("square", "∎"), ("blacksquare", "∎"), ("qed", "∎"), ("mid", "|"),
            ("langle", "⟨"), ("rangle", "⟩"), ("lfloor", "⌊"), ("rfloor", "⌋"),
            ("lceil", "⌈"), ("rceil", "⌉"), ("top", "⊤"), ("bot", "⊥"),
        ]
        .into_iter()
        .collect()
    })
}

/// Commands that are pure typesetting and carry no meaning in plain text.
const DROPPED: &[&str] = &[
    "left", "right", "displaystyle", "textstyle", "limits", "nolimits",
    "big", "Big", "bigg", "Bigg", "bigl", "bigr", "Bigl", "Bigr",
    "biggl", "biggr", "Biggl", "Biggr", "middle", "phantom",
    "quad", "qquad", "," , ";", ":", "!", " ",
];

/// Commands of the form `\cmd{...}` whose braces are decoration: the content
/// is kept and the wrapper discarded.
const UNWRAPPED: &[&str] = &[
    "text", "mathrm", "mathbf", "mathit", "mathsf", "mathtt", "boxed",
    "operatorname", "textbf", "textit", "mbox",
    // Fonts. The named entries above catch ℝ, ℕ and the rest; anything else
    // falls back to the plain letter, which reads better than a glyph half
    // the terminals in use would render as a box.
    "mathbb", "mathcal", "mathfrak", "mathscr", "mathbfcal",

];

/// Accents carry mathematical meaning even without a portable combining glyph.
const ACCENTS: &[&str] = &[
    "bar", "hat", "vec", "tilde", "dot", "ddot", "check", "breve", "acute",
    "grave", "overline", "underline", "widehat", "widetilde", "overrightarrow",
    "underbrace", "overbrace",
];

/// Function names LaTeX writes as commands. Only the backslash goes.
const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan",
    "sinh", "cosh", "tanh", "log", "ln", "exp", "lim", "max", "min", "sup",
    "inf", "det", "dim", "ker", "deg", "gcd", "lcm", "bmod", "mod",
];

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴',
        '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹',
        '+' => '⁺', '-' | '−' => '⁻', '=' => '⁼', '(' => '⁽', ')' => '⁾',
        'n' => 'ⁿ', 'i' => 'ⁱ', 'a' => 'ᵃ', 'b' => 'ᵇ', 'c' => 'ᶜ',
        'd' => 'ᵈ', 'e' => 'ᵉ', 'f' => 'ᶠ', 'g' => 'ᵍ', 'h' => 'ʰ',
        'j' => 'ʲ', 'k' => 'ᵏ', 'l' => 'ˡ', 'm' => 'ᵐ', 'o' => 'ᵒ',
        'p' => 'ᵖ', 'r' => 'ʳ', 's' => 'ˢ', 't' => 'ᵗ', 'u' => 'ᵘ',
        'v' => 'ᵛ', 'w' => 'ʷ', 'x' => 'ˣ', 'y' => 'ʸ', 'z' => 'ᶻ',
        // `x^\top` is transpose far more often than it is the logic symbol.
        '⊤' | 'T' => 'ᵀ',
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄',
        '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉',
        '+' => '₊', '-' | '−' => '₋', '=' => '₌', '(' => '₍', ')' => '₎',
        'a' => 'ₐ', 'e' => 'ₑ', 'h' => 'ₕ', 'i' => 'ᵢ', 'j' => 'ⱼ',
        'k' => 'ₖ', 'l' => 'ₗ', 'm' => 'ₘ', 'n' => 'ₙ', 'o' => 'ₒ',
        'p' => 'ₚ', 'r' => 'ᵣ', 's' => 'ₛ', 't' => 'ₜ', 'u' => 'ᵤ',
        'v' => 'ᵥ', 'x' => 'ₓ',
        _ => return None,
    })
}

// ─── Expression conversion ──────────────────────────────────────────────────

/// Read a `{...}` group starting at `chars[i]`, returning its contents and the
/// index just past the closing brace. Nested braces are respected.
fn read_group(chars: &[char], i: usize) -> Option<(String, usize)> {
    read_delimited(chars, i, '{', '}')
}

fn read_delimited(chars: &[char], i: usize, open: char, close: char) -> Option<(String, usize)> {
    if chars.get(i) != Some(&open) { return None; }
    let mut depth = 1usize;
    let mut j = i + 1;
    while j < chars.len() {
        if chars[j] == '\\' { j += 2; continue; }
        if chars[j] == open { depth += 1; }
        if chars[j] == close {
            depth -= 1;
            if depth == 0 { return Some((chars[i+1..j].iter().collect(), j + 1)); }
        }
        j += 1;
    }
    None
}

/// Include attached arguments in an unsupported command's literal fallback.
fn command_end(chars: &[char], after: usize) -> usize {
    let mut end = after;
    loop {
        let mut next = end;
        while chars.get(next).is_some_and(|c| c.is_whitespace()) { next += 1; }
        let delimiters = match chars.get(next) {
            Some('{') => ('{', '}'), Some('[') => ('[', ']'), _ => return end,
        };
        match read_delimited(chars, next, delimiters.0, delimiters.1) {
            Some((_, stop)) => end = stop,
            None => return chars.len(),
        }
    }
}

/// The argument of a command: a braced group, a whole command, or the single
/// next character.
///
/// The command case matters more than it looks: `x^\top`, `\int_0^\infty`
/// and `\oint_\gamma` are all written without braces, and taking one
/// character left the exponent as a lone backslash — `x^(\)top`.
fn read_argument(chars: &[char], i: usize) -> Option<(String, usize)> {
    read_argument_depth(chars, i, 0)
}

fn read_argument_depth(chars: &[char], i: usize, depth: usize) -> Option<(String, usize)> {
    if depth > 32 { return None; }
    // A space between a command and its argument is not part of it:
    // `\pmod n` takes `n`, not the space.
    let i = {
        let mut j = i;
        while chars.get(j) == Some(&' ') {
            j += 1;
        }
        j
    };
    if let Some(group) = read_group(chars, i) {
        return Some(group);
    }
    if chars.get(i) == Some(&'\\') {
        let (name, after) = read_command(chars, i + 1);
        let mut end = after;
        if name == "sqrt" && chars.get(end) == Some(&'[') {
            end = read_delimited(chars, end, '[', ']')?.1;
        }
        let arguments = if matches!(name.as_str(), "frac" | "dfrac" | "tfrac" | "binom" | "dbinom" | "tbinom") { 2 }
            else if UNWRAPPED.contains(&name.as_str()) || ACCENTS.contains(&name.as_str())
                || matches!(name.as_str(), "sqrt" | "pmod" | "xrightarrow" | "xleftarrow") { 1 }
            else { 0 };
        for _ in 0..arguments {
            end = read_argument_depth(chars, end, depth + 1)?.1;
        }
        if !known_command(&name) { end = command_end(chars, end); }
        return Some((chars[i..end].iter().collect(), end));
    }
    chars.get(i).map(|c| (c.to_string(), i + 1))
}

/// Map every character through `f`, or give up and return `None`.
fn map_script(text: &str, f: fn(char) -> Option<char>) -> Option<String> {
    text.chars().map(f).collect()
}

/// Wrap in parentheses when the expression would otherwise re-associate —
/// `\frac{a+b}{c}` must not become `a+b/c`.
fn parenthesize(s: &str) -> String {
    // One character cannot re-associate, whatever it is: `x^∞`, not `x^(∞)`.
    if s.chars().count() <= 1 {
        return s.to_string();
    }
    let atomic = s.parse::<f64>().is_ok();
    if atomic || already_wrapped(s) {
        s.to_string()
    } else {
        format!("({s})")
    }
}

/// Is the whole expression inside one pair of parentheses?
///
/// Testing only the first and last characters is not enough: in
/// `(-5)^2-4(1)(6)` the opening paren closes long before the end, so treating
/// it as wrapped drops the parentheses a `\sqrt` needed to show its extent.
fn already_wrapped(s: &str) -> bool {
    if !(s.starts_with('(') && s.ends_with(')')) {
        return false;
    }
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                // Back to nothing before the end: the pair does not span it.
                if depth == 0 && i + c.len_utf8() != s.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// Convert one LaTeX expression to plain text. Recursive: arguments are
/// converted before being placed.
pub fn convert(expr: &str) -> String {
    if expr.len() > 16 * 1024 { return expr.into(); }
    // Recursive conversion is bounded, and malformed grouping stays literal.
    let mut depth = 0usize;
    let mut escaped = false;
    for c in expr.chars() {
        if escaped { escaped = false; continue; }
        if c == '\\' { escaped = true; continue; }
        if c == '{' { depth += 1; }
        if c == '}' {
            if depth == 0 { return expr.into(); }
            depth -= 1;
        }
        if depth > 32 { return expr.into(); }
    }
    if depth != 0 { return expr.into(); }
    if expr.contains(r"\begin{") { return crate::latex_layout::render(expr); }
    let chars: Vec<char> = expr.chars().collect();
    let mut out = String::new();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '\\' => {
                // `\\` is a line break inside an environment.
                if chars.get(i + 1) == Some(&'\\') {
                    out.push('\n');
                    i += 2;
                    continue;
                }
                let (name, after) = read_command(&chars, i + 1);
                if !known_command(&name) { return expr.into(); }
                i = apply_command(&chars, &name, after, &mut out);
            }
            '^' | '_' => {
                let sup = chars[i] == '^';
                match read_argument(&chars, i + 1) {
                    Some((arg, next)) => {
                        let inner = convert(&arg);
                        let mapped = if sup {
                            map_script(&inner, superscript)
                        } else {
                            map_script(&inner, subscript)
                        };
                        match mapped {
                            Some(script) => out.push_str(&script),
                            // No Unicode form: keep the notation rather than
                            // silently dropping the exponent.
                            None => {
                                out.push(if sup { '^' } else { '_' });
                                out.push_str(&parenthesize(&inner));
                            }
                        }
                        i = next;
                    }
                    None => {
                        out.push(chars[i]);
                        i += 1;
                    }
                }
            }
            '{' | '}' => i += 1, // grouping braces carry no meaning in text
            // Alignment markers separate matrix columns and aligned-equation
            // parts; dropping them outright ran `2 & 1` together as `21`.
            '&' => {
                out.push(' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    collapse_spaces(&out)
}

/// Squeeze runs of spaces left behind by spacing commands and dropped markup.
/// Newlines are preserved: they separate rows and aligned lines.
fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = false;
    for c in s.chars() {
        let is_space = c == ' ';
        if !(is_space && last_was_space) {
            out.push(c);
        }
        last_was_space = is_space;
    }
    out
}

/// Read the command name at `i`: letters, or a single punctuation character
/// for spacing commands like `\,`.
fn read_command(chars: &[char], i: usize) -> (String, usize) {
    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_alphabetic() {
        j += 1;
    }
    if j == i {
        // A one-character command.
        return (chars.get(i).map(|c| c.to_string()).unwrap_or_default(), i + 1);
    }
    (chars[i..j].iter().collect(), j)
}

fn known_command(name: &str) -> bool {
    symbols().contains_key(name) || UNWRAPPED.contains(&name) || ACCENTS.contains(&name)
        || FUNCTIONS.contains(&name) || DROPPED.contains(&name)
        || matches!(name, "begin" | "end" | "frac" | "dfrac" | "tfrac" | "sqrt"
            | "pmod" | "binom" | "dbinom" | "tbinom" | "xrightarrow" | "xleftarrow"
            | "{" | "}" | "$" | "%" | "&" | "#" | "_")
}

/// Emit the replacement for `\name` and return the index to continue from.
fn apply_command(chars: &[char], name: &str, after: usize, out: &mut String) -> usize {
    // `\begin{env}` / `\end{env}`: the environment name is not content.
    if name == "begin" || name == "end" {
        if let Some((env, next)) = read_group(chars, after) {
            if matches!(env.as_str(), "matrix" | "pmatrix" | "bmatrix" | "Bmatrix" | "vmatrix" | "Vmatrix" | "aligned" | "align" | "align*" | "cases" | "equation" | "equation*") {
                return next;
            }
        }
    }

    if name == "frac" || name == "dfrac" || name == "tfrac" {
        if let Some((num, i1)) = read_argument(chars, after) {
            if let Some((den, i2)) = read_argument(chars, i1) {
                out.push_str(&parenthesize(&convert(&num)));
                out.push('/');
                out.push_str(&parenthesize(&convert(&den)));
                return i2;
            }
        }
    }

    if name == "sqrt" {
        let mut start = after;
        while chars.get(start).is_some_and(|c| c.is_whitespace()) { start += 1; }
        let mut index = None;
        if chars.get(start) == Some(&'[') {
            if let Some((value, next)) = read_delimited(chars, start, '[', ']') {
                index = Some(value); start = next;
            } else {
                out.extend(chars[after.saturating_sub(name.len() + 1)..].iter());
                return chars.len();
            }
        }
        if let Some((arg, next)) = read_argument(chars, start) {
            let inner = convert(&arg);
            match index.as_deref().map(str::trim) {
                None | Some("2") => { out.push('√'); out.push_str(&parenthesize(&inner)); }
                Some("3") => { out.push('∛'); out.push_str(&parenthesize(&inner)); }
                Some("4") => { out.push('∜'); out.push_str(&parenthesize(&inner)); }
                Some(n) => out.push_str(&format!("root({}, {})", convert(n), inner)),
            }
            return next;
        }
    }

    if ACCENTS.contains(&name) {
        if let Some((arg, next)) = read_argument(chars, after) {
            out.push_str(&format!("{name}({})", convert(&arg)));
            return next;
        }
    }

    // Escaped punctuation stands for itself: `\{`, `\%`, `\$`.
    if matches!(name, "{" | "}" | "$" | "%" | "&" | "#" | "_") {
        out.push_str(name);
        return after;
    }

    if name == "pmod" {
        if let Some((arg, next)) = read_argument(chars, after) {
            out.push_str(&format!("(mod {})", convert(&arg)));
            return next;
        }
    }

    if name == "binom" || name == "dbinom" || name == "tbinom" {
        if let Some((n, i1)) = read_argument(chars, after) {
            if let Some((k, i2)) = read_argument(chars, i1) {
                out.push_str(&format!("C({}, {})", convert(&n), convert(&k)));
                return i2;
            }
        }
    }

    // `\xrightarrow{d}` — the arrow is the meaning, the label decoration.
    if name.starts_with("xrightarrow") || name.starts_with("xleftarrow") {
        let next = read_argument(chars, after).map(|(_, n)| n).unwrap_or(after);
        out.push_str(if name.starts_with("xr") { "→" } else { "←" });
        return next;
    }

    // `\mathbb{R}` and friends are looked up whole, before the font commands
    // below strip their braces.
    if let Some((arg, next)) = read_group(chars, after) {
        let combined = format!("{name}{{{arg}}}");
        if let Some(sym) = symbols().get(combined.as_str()) {
            out.push_str(sym);
            return next;
        }
    }

    if UNWRAPPED.contains(&name) {
        if let Some((arg, next)) = read_argument(chars, after) {
            out.push_str(&convert(&arg));
            return next;
        }
    }

    if let Some(sym) = symbols().get(name) {
        out.push_str(sym);
        return after;
    }

    if FUNCTIONS.contains(&name) {
        out.push_str(name);
        return after;
    }

    if DROPPED.contains(&name) {
        // Spacing commands become a space; the rest vanish.
        if matches!(name, "quad" | "qquad" | "," | ";" | ":" | " ") {
            out.push(' ');
        }
        return after;
    }

    // Unknown: leave it visible rather than guess.
    out.push('\\');
    out.push_str(name);
    let end = command_end(chars, after);
    out.extend(chars[after..end].iter());
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_symbols_and_scripts() {
        assert_eq!(convert(r"x^2 + y^2 = z^2"), "x² + y² = z²");
        assert_eq!(convert(r"\alpha + \beta \leq \gamma"), "α + β ≤ γ");
        assert_eq!(convert(r"a_1 + a_2"), "a₁ + a₂");
        // π has no superscript form, so the whole exponent falls back to
        // `^` notation rather than half of it being raised.
        assert_eq!(convert(r"e^{i\pi} + 1 = 0"), "e^(iπ) + 1 = 0");
    }

    #[test]
    fn converts_fractions_and_roots() {
        assert_eq!(convert(r"\frac{1}{2}"), "1/2");
        assert_eq!(convert(r"\frac{a+b}{c}"), "(a+b)/c");
        assert_eq!(convert(r"\sqrt{2}"), "√2");
        assert_eq!(convert(r"\sqrt{b^2-4ac}"), "√(b²-4ac)");
        // The leading paren closes before the end, so the root still needs
        // its own to show how far it reaches.
        assert_eq!(convert(r"\sqrt{(-5)^2-4(1)(6)}"), "√((-5)²-4(1)(6))");
        // A genuinely wrapped expression is not wrapped twice.
        assert_eq!(convert(r"\sqrt{(a+b)}"), "√(a+b)");
    }

    #[test]
    fn drops_typesetting_and_keeps_content() {
        assert_eq!(convert(r"\left( x \right)"), "( x )");
        assert_eq!(convert(r"\biggl(\sum a_i\biggr)^2"), "(∑ aᵢ)²");
        assert_eq!(convert(r"\boxed{x = 8}"), "x = 8");
        assert_eq!(convert(r"\text{if } x > 0"), "if x > 0");
        assert_eq!(convert(r"\int_0^1 x\,dx"), "∫₀¹ x dx");
    }

    /// An exponent with no Unicode form must stay legible, not disappear.
    /// A command is one argument. Without this, `x^\top` took a lone
    /// backslash as the exponent and rendered as `x^(\)top`.
    #[test]
    fn a_command_is_a_single_argument() {
        assert_eq!(convert(r"x^\top A x"), "xᵀ A x");
        assert_eq!(convert(r"\int_0^\infty f"), "∫₀^∞ f");
        assert_eq!(convert(r"\oint_\gamma g"), "∮_γ g");
    }

    #[test]
    fn unwraps_fonts_and_accents_it_cannot_draw() {
        assert_eq!(convert(r"\mathbb{E}[X]"), "E[X]");
        assert_eq!(convert(r"\mathcal{N}(\mu,\sigma^2)"), "N(μ,σ²)");
        // The named ones keep their glyph.
        assert_eq!(convert(r"\mathbb{R}"), "ℝ");
        assert_eq!(convert(r"\bar{X}_n"), "bar(X)ₙ");
        assert_eq!(convert(r"\overline{AB}"), "overline(AB)");
    }

    #[test]
    fn handles_escapes_and_modular_notation() {
        assert_eq!(convert(r"\{x \mid x \in A\}"), "{x | x ∈ A}");
        // The space before the argument is separation, not the argument.
        assert_eq!(convert(r"c \equiv m^e \pmod n"), "c ≡ mᵉ (mod n)");
        assert_eq!(convert(r"\binom{n}{k}"), "C(n, k)");
    }

    /// An exponent every character of which has a Unicode form is raised
    /// whole, sign and all.
    #[test]
    fn raises_whole_exponents() {
        assert_eq!(convert(r"x^{2n+1}"), "x²ⁿ⁺¹");
        assert_eq!(convert(r"a_{i+1}"), "aᵢ₊₁");
    }

    /// One character without a Unicode form takes the whole group with it,
    /// into notation that is still readable.
    #[test]
    fn keeps_notation_it_cannot_map() {
        assert_eq!(convert(r"x^{q+1}"), "x^(q+1)");
        assert_eq!(convert(r"\unknowncmd{x}"), r"\unknowncmd{x}");
    }

    /// Column separators are structure, not decoration.
    #[test]
    fn keeps_matrix_columns_apart() {
        let got = convert(r"\begin{pmatrix}2&1\\1&2\end{pmatrix}");
        assert!(got.contains("2  1"), "{got:?}");
        assert!(got.contains("1  2"), "{got:?}");
        assert!(!got.contains("21"), "columns ran together: {got:?}");
    }

    #[test]
    fn handles_aligned_environments() {
        let got = convert(r"\begin{aligned} a &= b \\ c &= d \end{aligned}");
        assert!(got.contains("a = b"), "{got:?}");
        assert!(got.contains("c = d"), "{got:?}");
    }
}

// ─── Finding math in markdown ───────────────────────────────────────────────

/// Rewrite the math in a markdown document, leaving everything else alone.
///
/// Code is skipped: a fenced block or a backtick span may legitimately contain
/// `$` or backslashes, and rewriting those would corrupt the very thing the
/// user asked to see verbatim.
pub fn render_math(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prose = String::new();
    let mut in_fence = false;

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            // Flush the prose before the fence: display blocks are matched
            // across lines, so they need a whole region at a time.
            out.push_str(&render_prose(&std::mem::take(&mut prose)));
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
        } else {
            prose.push_str(line);
        }
    }
    out.push_str(&render_prose(&prose));
    out
}

/// Display blocks first, since they may span lines, then the inline spans on
/// what is left.
fn render_prose(chunk: &str) -> String {
    render_display_blocks(chunk)
        .split_inclusive('\n')
        .map(render_line)
        .collect()
}

/// Delimiters that open a block spanning as many lines as it needs.
const BLOCK_DELIMITERS: &[(&str, &str)] = &[("$$", "$$"), (r"\[", r"\]")];

fn render_display_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(m) = first_match(rest, BLOCK_DELIMITERS, |body| {
        !body.trim().is_empty() && body.len() <= 4000
    }) {
        out.push_str(&rest[..m.start]);
        out.push_str(convert(m.body).trim());
        // A block that sat on its own line keeps its line, and the newline it
        // left behind is consumed rather than doubled.
        let standalone = rest[..m.start].ends_with('\n') || m.start == 0;
        rest = &rest[m.end..];
        if standalone {
            rest = rest.strip_prefix('\n').unwrap_or(rest);
            out.push('\n');
        }
    }
    out.push_str(rest);
    out
}

/// A delimited span: where it sits, and what is between the delimiters.
struct Span<'a> {
    start: usize,
    end: usize,
    body: &'a str,
}

/// The earliest delimited span in `text` that `accept` approves.
///
/// Earliest, not first-delimiter-that-matches: scanning the delimiter list in
/// order and taking the first one with a match anywhere lets a `$$` later in
/// the document swallow a `\[` block that came before it.
fn first_match<'a>(
    text: &'a str,
    delimiters: &[(&str, &str)],
    accept: impl Fn(&str) -> bool,
) -> Option<Span<'a>> {
    let mut best: Option<Span<'a>> = None;
    for (open, close) in delimiters {
        let mut from = 0usize;
        while let Some(rel) = text[from..].find(open) {
            let start = from + rel;
            let after = start + open.len();
            let Some(len) = text[after..].find(close) else { break };
            let body = &text[after..after + len];
            if accept(body) {
                if best.as_ref().is_none_or(|b| start < b.start) {
                    best = Some(Span { start, end: after + len + close.len(), body });
                }
                break;
            }
            from = after;
        }
    }
    best
}

/// Rewrite math outside of inline-code spans on one line.
fn render_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;

    while let Some(tick) = rest.find('`') {
        out.push_str(&render_spans(&rest[..tick]));
        // Copy the code span through untouched, including its backticks.
        match rest[tick + 1..].find('`') {
            Some(end) => {
                let close = tick + 1 + end + 1;
                out.push_str(&rest[tick..close]);
                rest = &rest[close..];
            }
            None => {
                out.push_str(&rest[tick..]);
                return out;
            }
        }
    }
    out.push_str(&render_spans(rest));
    out
}

/// The delimiters, longest first so `$$` is not read as two `$`.
const DELIMITERS: &[(&str, &str)] = &[("$$", "$$"), (r"\[", r"\]"), (r"\(", r"\)"), ("$", "$")];

fn render_spans(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(m) = first_math_span(rest) {
        out.push_str(&rest[..m.start]);
        out.push_str(convert(m.body).trim());
        rest = &rest[m.end..];
    }
    out.push_str(rest);
    out
}

/// Like [`first_match`], but the acceptance test depends on which delimiter
/// opened the span — only `$` is ambiguous enough to need vetting.
fn first_math_span(text: &str) -> Option<Span<'_>> {
    let mut best: Option<Span<'_>> = None;
    for (open, close) in DELIMITERS {
        if let Some(span) = first_match(text, &[(open, close)], |body| looks_like_math(body, open)) {
            if best.as_ref().is_none_or(|b| span.start < b.start) {
                best = Some(span);
            }
        }
    }
    best
}

/// Is this delimited span really maths?
///
/// `\(`, `\[` and `$$` are unambiguous. A single `$` is not: shell variables,
/// prices and option flags all use it, and rewriting `costs $5 to $10` as
/// maths would be worse than leaving the LaTeX alone. So a `$…$` span has to
/// look like an expression — a command, a script, or an operator applied to
/// something with a letter in it.
fn looks_like_math(body: &str, open: &str) -> bool {
    if body.is_empty() || body.len() > 400 {
        return false;
    }
    if open != "$" {
        return true;
    }
    if body.contains('\n') || body.contains('$') {
        return false;
    }
    if body.contains('\\') || body.contains('^') || body.contains('_') {
        return true;
    }
    // A bare variable: `$n$`, `$f$`.
    if body.chars().count() <= 3 && body.chars().all(|c| c.is_alphanumeric()) {
        return true;
    }
    // Prose reads as words. A run of two or more letters is a word, not a
    // variable, and treating "costs $5 to $10" as maths would delete the
    // dollar signs from a sentence about money — worse than leaving an
    // interval like `$(a, b)$` unconverted, which is only cosmetic.
    if longest_letter_run(body) >= 2 {
        return body.chars().any(|c| "=+-*/<>".contains(c));
    }
    // Single letters and punctuation: `$(a, b)$`, `$x = 1$`, `$f(x)$`.
    body.chars().any(|c| c.is_ascii_alphabetic())
}

fn longest_letter_run(s: &str) -> usize {
    let mut best = 0;
    let mut run = 0;
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    best
}

#[cfg(test)]
mod markdown_tests {
    use super::*;

    #[test]
    fn rewrites_every_delimiter_form() {
        assert_eq!(render_math(r"Let \(f(x)=x^2\)."), "Let f(x)=x².");
        // A display block keeps its own line.
        assert_eq!(render_math(r"\[ \alpha^2 \]"), "α²\n");
        assert_eq!(render_math(r"$$x_1 + x_2$$"), "x₁ + x₂\n");
        assert_eq!(render_math(r"the value $x^2$ here"), "the value x² here");
    }

    /// Code is the one place a backslash or `$` means itself.
    #[test]
    fn leaves_code_alone() {
        let fenced = "```sh\necho $HOME \\alpha\n```\n";
        assert_eq!(render_math(fenced), fenced);
        assert_eq!(
            render_math(r"run `echo $x^2$` first"),
            r"run `echo $x^2$` first"
        );
    }

    /// Prices and shell variables are not equations.
    #[test]
    fn does_not_treat_prose_dollars_as_math() {
        assert_eq!(render_math("costs $5 to $10 today"), "costs $5 to $10 today");
        assert_eq!(render_math("set $PATH and $HOME now"), "set $PATH and $HOME now");
        assert_eq!(render_math("paid $5, or $10 total"), "paid $5, or $10 total");
    }

    /// Intervals and applications carry no operator, but they are still maths.
    #[test]
    fn accepts_spans_of_single_letters_and_punctuation() {
        assert_eq!(render_math(r"on $(a, b)$ and"), "on (a, b) and");
        assert_eq!(render_math(r"where $f(x)$ is"), "where f(x) is");
        assert_eq!(render_math(r"for $n$ terms"), "for n terms");
    }

    #[test]
    fn handles_a_display_block_across_lines() {
        let got = render_math("Result:\n$$\n\\frac{1}{2}\n$$\n");
        assert!(got.contains("1/2"), "{got:?}");
        assert!(!got.contains('$'), "{got:?}");
    }
}



#[cfg(test)]
mod correctness_tests {
    use super::*;
    #[test]
    fn groups_products_and_compound_command_arguments() {
        assert_eq!(convert(r"\frac{1}{2a}"), "1/(2a)");
        assert_eq!(convert(r"\frac{1}{23}"), "1/23");
        assert_eq!(convert(r"\sqrt{ab}"), "√(ab)");
        assert_eq!(convert(r"x^\frac{1}{2}"), "x^(1/2)");
        assert_eq!(convert(r"\frac\sqrt{x}{y}"), "(√x)/y");
    }
    #[test]
    fn indexed_roots_and_accents_keep_meaning() {
        assert_eq!(convert(r"\sqrt[3]{x+1}"), "∛(x+1)");
        assert_eq!(convert(r"\sqrt[4]{x}"), "∜x");
        assert_eq!(convert(r"\sqrt[n]{x}"), "root(n, x)");
        assert_eq!(convert(r"\vec{v} + \bar{z}"), "vec(v) + bar(z)");
    }
    #[test]
    fn unsupported_commands_and_invalid_groups_stay_literal() {
        for text in [r"\mystery[opt]{x+y}{z}", r"\alpha + \mystery{a  +  b}", r"\mystery{a \{ b \} c}", r"\frac{1}{", r"\sqrt[3{x}"] {
            assert_eq!(convert(text), text, "{text}");
        }
        let deep = format!("{}x{}", "{".repeat(1000), "}".repeat(1000));
        assert_eq!(convert(&deep), deep);
    }
}

/// Display math stays separate from Markdown so alignment and fallback source
/// cannot be interpreted as emphasis, tables, or lists.
pub(crate) enum MathPart<'a> { Prose(&'a str), Display(&'a str) }

pub(crate) fn math_regions(text: &str) -> (Vec<MathPart<'_>>, Option<usize>) {
    let mut parts = Vec::new();
    let mut from = 0;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with('`') {
            let count = rest.bytes().take_while(|b| *b == b'`').count();
            let delimiter = "`".repeat(count);
            if let Some(end) = rest[count..].find(&delimiter) {
                i += count + end + count;
                continue;
            }
            break; // unfinished inline code remains literal
        }
        let delimiter = if rest.starts_with("$$") { Some(("$$", "$$", true)) }
            else if rest.starts_with(r"\[") { Some((r"\[", r"\]", true)) }
            else if rest.starts_with(r"\(") { Some((r"\(", r"\)", false)) }
            else if rest.starts_with('$') { Some(("$", "$", false)) }
            else { None };
        let found = if let Some((open, close, display)) = delimiter {
            if let Some(end) = rest[open.len()..].find(close) {
                let start = i + open.len();
                let stop = start + end;
                let next = stop + close.len();
                if open == "$" && !looks_like_math(&text[start..stop], open) {
                    i += 1; continue;
                }
                if display || text[start..stop].contains(r"\begin{") {
                    Some((start, stop, next))
                } else { i = next; continue; }
            } else if display {
                parts.push(MathPart::Prose(&text[from..]));
                return (parts, Some(i));
            } else { i += open.len(); continue; }
        } else if rest.starts_with(r"\begin{") {
            if let Some(env) = crate::latex_layout::environment_at(text, i) {
                Some((i, env.end, env.end))
            } else {
                parts.push(MathPart::Prose(&text[from..]));
                return (parts, Some(i));
            }
        } else { None };
        if let Some((start, stop, next)) = found {
            if from < i { parts.push(MathPart::Prose(&text[from..i])); }
            parts.push(MathPart::Display(&text[start..stop]));
            from = next; i = next;
        } else {
            let c = rest.chars().next().unwrap();
            i += c.len_utf8();
            if c == '\\' {
                if let Some(next) = text[i..].chars().next() { i += next.len_utf8(); }
            }
        }
    }
    if from < text.len() { parts.push(MathPart::Prose(&text[from..])); }
    (parts, None)
}
