#!/usr/bin/env python3

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
INPUTS = DOCS / "topside-inputs.md"
LEADER = DOCS / "snorkel-on-grok-leader.md"
README_EN = ROOT / "README.md"
README_ZH = ROOT / "README.zh-CN.md"

STALE = [
    "本仓库还没有 `snorkel` 或 `topside` 二进制",
    "control-aqualung doctor 现在退出 2",
    "你接下来要实现 `snorkel` 和 `topside`",
    "验证图写的是 topside 立刻回忙",
    "`control-aqualung doctor` 在只有 `snorkel` 时退出 1",
    "本页没有展开那个常量的毫秒数",
    "status line 键",
    "`authMethods` 为空数组表示 Agent 没有可走的 ACP 认证方法。",
]

REQUIRED_H2 = [
    "来源",
    "aqualung 已写明的合同",
    "snorkel 拨号合同",
    "1943 上的 TLS",
    "Leader 帧",
    "请求 id 改写",
    "会话订阅、先答、掉线",
    "initialize 与能力注入",
    "grok agent serve 不是这条路",
    "7678 上的手机 ACP",
    "本页未指定的事项",
]

REQUIRED_SNIPPETS = [
    '"stage": "design"',
    '"stage": "incomplete"',
    "clientStatusLine",
    "LEADER_READY_TIMEOUT",
    "120 秒",
]


def fail(message: str) -> None:
    print(f"FAIL: {message}", file=sys.stderr)
    raise SystemExit(1)


def main() -> None:
    scanned = [INPUTS, LEADER, README_EN, README_ZH]
    for path in scanned:
        if not path.is_file():
            fail(f"missing {path.relative_to(ROOT)}")
        text = path.read_text(encoding="utf-8")
        for phrase in STALE:
            if phrase in text:
                fail(f"{path.relative_to(ROOT)} still contains {phrase!r}")

    text = INPUTS.read_text(encoding="utf-8")
    headings = re.findall(r"^## (.+)$", text, flags=re.M)
    for heading in REQUIRED_H2:
        if heading not in headings:
            fail(f"{INPUTS.relative_to(ROOT)} missing H2 {heading!r}")

    for snippet in REQUIRED_SNIPPETS:
        if snippet not in text:
            fail(f"{INPUTS.relative_to(ROOT)} missing {snippet!r}")

    for readme in [README_EN, README_ZH]:
        body = readme.read_text(encoding="utf-8")
        if "docs/topside-inputs.md" not in body:
            fail(f"{readme.relative_to(ROOT)} does not link docs/topside-inputs.md")

    blocks = re.findall(r"```json\n(.*?)```", text, flags=re.S)
    if not blocks:
        fail(f"{INPUTS.relative_to(ROOT)} has no json fences")
    for index, block in enumerate(blocks, start=1):
        try:
            json.loads(block)
        except json.JSONDecodeError as error:
            fail(f"json fence {index} does not parse: {error}")

    print(
        f"ok: {len(REQUIRED_H2)} headings, {len(blocks)} json fences, "
        f"{len(scanned)} files, no stale phrases"
    )


if __name__ == "__main__":
    main()
