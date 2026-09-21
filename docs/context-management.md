# Context management — MyCLI 1.9.8

## Audit findings and fixes

The previous runner tracked provider input tokens and compaction failures inside one
`run_agent_streaming` call. Both reset on the next user prompt, so ordinary chat could
fill the window without triggering compaction. The SDK's `compact_threshold` setting
was also ignored. State now persists for the lifetime of the agent and is reset when
switching models/providers through the existing agent rebuild.

Other fixes:

- Trigger before requests using the usable input budget: window minus `max_tokens`
  minus 1,024 tokens of framing headroom. The default threshold is 90%.
- Include system prompts, tool schemas, structured calls/results, and history in
  accounting. Anchor estimates to the last reported provider input count; conservatively
  count newly added serialized bytes as tokens. Before a measurement, use bytes as an
  upper-bound heuristic. This is not an exact tokenizer, especially for attachments.
  Changing reasoning effort or thinking mode invalidates the previous measurement.
- Summarize all older textual evidence in bounded sequential chunks, rather than
  taking a tiny excerpt of the beginning/end. Carry the preceding summary into each
  chunk. Preserve call names, arguments, result text, paths, goals, and constraints.
- Keep a recent coherent turn; normally prefer ten recent messages for automatic
  compaction and two for manual compaction. Large tails reduce the preference to two.
  Safe boundaries may keep more messages. For long single-turn tool loops, use a
  boundary between complete tool rounds. Merge adjacent user content when necessary.
- Inherit model reasoning/thinking/sampling settings for summary requests. Bound summary
  output to the smallest of 4,096 tokens, configured `max_tokens`, and one eighth of the
  resolved window. No tools are supplied to the summarizer.
- Apply history replacement only after a complete, nonempty summary reduces the
  estimated size. Failed or cancelled summaries do not clear the conversation.
  Completed summary calls contribute to session usage, including before a later failure.
- Preserve session totals in the footer, discard stale cache-prefix estimates after
  compaction, and avoid treating cumulative multi-call usage as current context size.
- Load persisted history only when the agent has no in-memory history; repeated turns
  no longer append the same stored history again.

## Manual command

`/compact [optional focus]` performs compaction immediately, even below the automatic
threshold. `/compact status` shows the resolved window, usable input budget, estimated
current input, and message count. Both are available through command completion/help.
The summarizer can require multiple model calls and is cancellable with Esc/Ctrl+C.
It does not reset the conversation, change persona/provider, or execute tools.
With session persistence enabled by MyCLI, successful compaction appends a replacement
checkpoint to the journal. Earlier conversation entries remain in `transcript.jsonl`;
`context.json` materializes the compacted context for resumption. See [sessions](sessions.md).

Summarization is lossy. Older binary attachments and private reasoning blocks are not
copied into the text summary; retained recent messages preserve their original blocks.
Empty, truncated, failed, or oversized summaries retain the old history. If nothing
can be reduced safely, MyCLI reports a no-op. A provider error remains possible because
metadata and token estimates are not a universal tokenizer. There is no silent
history-dropping fallback.

## Model and deployment limits

1. Explicit `context_window` in the active configuration/profile wins.
2. Local metadata is inspected for positive `max_model_len`, `context_window`,
   `context_length`, then `max_context_length` (integer or decimal string).
3. A conservative model-family fallback applies when neither is available.

On 2026-09-21, the user's oMLX endpoint reported 262,144 for Qwen3.6-35B-A3B-8bit,
Cold-Fusion-709-L, and Gemma4-26B-A4B-it-8bit. These served limits take precedence
over guesses. Local names containing cloud nicknames such as Cold-Fusion's “Fable”
are no longer misclassified as Anthropic models. Unknown local model defaults remain
conservative; set a profile override when the server omits its serving limit.

Cloud fallback updates include [GPT-6 Astra's 1,050,000-token window](https://developers.openai.com/api/docs/models/gpt-6-astra)
and a conservative 1,000,000 for [DeepSeek Flash's documented 1M window](https://api-docs.deepseek.com/quick_start/pricing/).
A local proxy may serve those model IDs at a smaller limit; its reported or explicitly
configured limit wins. Older `deepseek-chat`/`deepseek-reasoner` aliases retain their
conservative fallback because local services may route them to older models.

## Upstream Cersei comparison

Reviewed revision `708c5055845ba6c682d92960cec99e3adbcca3e1`, without updating the SDK:

- [compact.rs](https://github.com/pacifio/cersei/blob/708c5055845ba6c682d92960cec99e3adbcca3e1/crates/cersei-agent/src/compact.rs)
  adds pair-aware splitting plus orphaned-result and unanswered-call diagnostics.
  We adapt the complete-tool-round boundary idea for long tool loops and make the
  exported snip helper pair-safe. We do not use snipping as the runtime fallback.
- [runner.rs](https://github.com/pacifio/cersei/blob/708c5055845ba6c682d92960cec99e3adbcca3e1/crates/cersei-agent/src/runner.rs)
  integrates warnings and an LLM-summary-to-snip fallback, but derives the window
  from model-name heuristics and estimates history text only. This would regress
  MyCLI's deployment-specific metadata handling and preservation on summary failures.
- Its summarizer still uses `get_all_text()`, fixed output tokens, zero temperature,
  and default provider options. Those omit structured tool evidence and do not carry
  the selected model's reasoning settings. MyCLI keeps the targeted implementation.
- `context_analyzer.rs` offers category breakdowns and strategy suggestions, but is
  heuristic advisory code; it is not the runtime budget used here.

Validation covers separate user turns, bounded multichunk summaries, structured tool
evidence, selected reasoning options, usage accounting, cancellation, failed/empty/
truncated summaries, persistent retry state, model aliases, and server metadata fields.
