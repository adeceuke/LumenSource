# Optional model performance testing

Installation validation and performance testing answer different questions:

- Installation validation confirms that the runtime, model identity, and a
  minimal inference request work.
- The optional performance test measures whether that valid installation feels
  responsive on the current machine. A slow or failed performance test never
  invalidates an installation.

## Standard workload

Version 1 performs a short warm-up followed by two measured requests. Measured
requests use a synthetic, offline passage, a 2,048-token context limit,
temperature zero, a fixed seed, reasoning disabled, and a maximum of 64 output
tokens. The median client-observed latency and combined runtime token counters
are used to reduce one-off noise without making CPU-only tests excessively long.

The shape follows the distinction made by
[`llama-bench`](https://github.com/ggml-org/llama.cpp/tree/master/tools/llama-bench)
between prompt processing and text generation. Lumen Source uses the installed
runtime rather than invoking `llama-bench`, so it measures the configuration the
user will actually run.

For Ollama, the final streaming response provides `load_duration`,
`prompt_eval_count`, `prompt_eval_duration`, `eval_count`, and `eval_duration` as
documented by the [Ollama chat API](https://docs.ollama.com/api/chat). Lumen
Source measures first-runtime-token and first-visible-token latency at the
client because Ollama does not report those durations separately.

## Interpretation

The initial experience thresholds are deliberately model-independent:

- Slow: first visible token above 20 seconds or generation below 5 tokens/s.
- Fast: first visible token at or below 5 seconds and generation at or above
  15 tokens/s.
- Balanced: results between those boundaries.

These are product experience thresholds, not claims about model correctness.
They should evolve through usability testing.

Suggestions are limited to compatible variants in the same catalog model
family. A local result calibrates candidate estimates using parameter count as
a conservative relative-compute signal. A smaller variant can be shown as a
faster alternative; the next larger variant can be shown as a quality-oriented
alternative only when its calibrated estimate remains responsive. Model size
is not presented as proof of quality.

Suggestions never download, replace, reconfigure, or remove a model. The user
must explicitly review the alternative through the normal installation wizard.

## CPU reporting

The test reports actual CPU, mixed, or GPU model allocation and intentionally
does not display whole-machine CPU utilization. Local inference runtimes can use
only a subset of available cores, making aggregate utilization confusing (for
example, one fully used core on a 16-core system appears as low overall usage).

## Persistence and invalidation

Results store the workload version, timestamp, model allocation, hardware
summary, and effective context. A future change should mark results stale when
the model digest, runtime version, workload version, hardware, or material load
settings change.
