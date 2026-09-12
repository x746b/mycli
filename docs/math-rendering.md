# Terminal math

MyCLI 1.8.0 preserves grouping and accents when converting LaTeX to Unicode:

| LaTeX | Terminal output |
|---|---|
| `e^{i\pi}` | `e^(iπ)` |
| `\frac{1}{2a}` | `1/(2a)` |
| `\sqrt[3]{x+1}` | `∛(x+1)` |
| `\vec{v} + \bar{z}` | `vec(v) + bar(z)` |

MyCLI 1.9.0 aligns display equations, matrices, and piecewise functions:

```text
  ⎡ a  b ⎤ ⎡ x ⎤ = ⎡ ax+by ⎤
  ⎣ c  d ⎦ ⎣ y ⎦   ⎣ cx+dy ⎦

     x = a_(jq) + 1
  long = b_(qr) + 2

  f(x) = ⎧ x²  x > 0
         ⎩ 0   x ≤ 0
```

Use `\[...\]` or `$$...$$` for display math. Bare `aligned`, `align`,
`equation`, `cases`, and matrix environments also render as display blocks.
Matrices support square, round, curly, determinant, or no brackets. Adjacent
matrices share a baseline; nested cells and Unicode column widths are supported.

Unsupported or malformed structures retain their source. Layouts wider than the
terminal fall back to a wrapped LaTeX source block. Layout parsing is limited to
4,000 bytes, 16 nesting levels, 64 rows, and 16 columns; oversized input remains
literal. This is a terminal preview, not a full TeX typesetter.

Fenced code stays literal, including `latex` fences. Display math bypasses Markdown
formatting to preserve its spacing and symbols. `MYCLI_RAW=1` still emits the
original model output.
