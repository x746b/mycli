#!/usr/bin/env python3
"""Replay a terminal byte stream onto a virtual screen and print the result.

Only the escapes mycli actually emits are modelled: cursor motion, erase,
save/restore, and DECSTBM with its cursor-homing side effect. Enough to see
what a session really looks like, which a raw capture cannot show.
"""
import sys, re

class Screen:
    def __init__(self, rows, cols):
        self.rows, self.cols = rows, cols
        self.buf = [[" "] * cols for _ in range(rows)]
        self.r = self.c = 0          # 0-indexed
        self.top, self.bot = 0, rows - 1
        self.saved = (0, 0)

    def scroll_up(self):
        del self.buf[self.top]
        self.buf.insert(self.bot, [" "] * self.cols)

    def lf(self):
        if self.r == self.bot:
            self.scroll_up()
        elif self.r < self.rows - 1:
            self.r += 1

    def put(self, ch):
        if self.c >= self.cols:
            self.c = 0
            self.lf()
        self.buf[self.r][self.c] = ch
        self.c += 1

CSI = re.compile(r"\x1b\[([0-9;?]*)([@-~])")

def replay(data, rows, cols):
    s = Screen(rows, cols)
    i = 0
    while i < len(data):
        ch = data[i]
        if ch == "\x1b":
            m = CSI.match(data, i)
            if m:
                params, final = m.group(1), m.group(2)
                nums = [int(p) for p in params.split(";") if p.isdigit()]
                n = nums[0] if nums else 0
                if final == "A": s.r = max(s.top, s.r - max(1, n))
                elif final == "B": s.r = min(s.bot, s.r + max(1, n))
                elif final == "C": s.c = min(cols - 1, s.c + max(1, n))
                elif final == "D": s.c = max(0, s.c - max(1, n))
                elif final == "H":
                    s.r = min(rows - 1, (nums[0] if nums else 1) - 1)
                    s.c = min(cols - 1, (nums[1] if len(nums) > 1 else 1) - 1)
                elif final == "K":
                    if n == 0: s.buf[s.r][s.c:] = [" "] * (cols - s.c)
                    elif n == 1: s.buf[s.r][:s.c + 1] = [" "] * (s.c + 1)
                    else: s.buf[s.r] = [" "] * cols
                elif final == "J":
                    if n == 2: s.buf = [[" "] * cols for _ in range(rows)]
                elif final == "r":
                    s.top = (nums[0] - 1) if nums else 0
                    s.bot = (nums[1] - 1) if len(nums) > 1 else rows - 1
                    s.r = s.top      # DECSTBM homes the cursor
                    s.c = 0
                elif final == "s": s.saved = (s.r, s.c)
                elif final == "u": s.r, s.c = s.saved
                i = m.end()
                continue
            i += 1
            continue
        if ch == "\n": s.lf()
        elif ch == "\r": s.c = 0
        elif ch == "\b": s.c = max(0, s.c - 1)
        elif ch == "\t": s.c = min(cols - 1, (s.c // 8 + 1) * 8)
        elif ch >= " ": s.put(ch)
        i += 1
    return s

rows, cols = int(sys.argv[1]), int(sys.argv[2])
data = sys.stdin.buffer.read().decode("utf-8", "replace")
s = replay(data, rows, cols)
for idx, line in enumerate(s.buf, 1):
    print(f"{idx:3d}|{''.join(line).rstrip()}")
print(f"--- cursor at row {s.r + 1}, col {s.c + 1}; region {s.top + 1}..{s.bot + 1}")
