![Orca Sequencer](assets/orca-sequencer.png)
*[Orca](https://github.com/hundredrabbits/Orca) - livecoding environment (reminds me of commit log)*

# GitHub Worklog

A CLI tool that fetches your daily GitHub commits, uses an LLM to summarize them into meaningful bullet points, and maintains a running log of your work.

## Features

- Fetches all commits across all your repositories for a given day (in your local timezone)
- Summarizes commits using cloud or local models (Claude, OpenAI, Gemini, or Ollama), falling back to the plain commit list if the model is unavailable
- Maintains a single markdown file with all daily recaps, newest first, written atomically
- Supports automated daily runs via macOS launchd

## Installation

### Prerequisites

- Rust (install via [rustup](https://rustup.rs/))
- [just](https://github.com/casey/just) command runner (`brew install just` or `cargo install just`)
- LLM access: Claude, OpenAI, Gemini, or Ollama

### Setup

```bash
cp .env.example .env      # Then edit with your credentials
just build
just install-hooks        # Optional: check formatting, lint and tests before each commit
```

## Usage

```bash
just                      # Show all available commands
just preview              # Preview today's recap without saving
just today                # Generate today's recap
just generate 2026-03-18  # Generate for a specific date
just week                 # Generate for the past 7 days
just config               # View configuration
just check-all            # Format check, clippy, tests
```

Existing entries are never overwritten by accident. To regenerate a day:

```bash
github-worklog generate --date 2026-03-18 --force
```

### CLI Flags

```bash
# Switch provider on the fly
github-worklog today --provider claude
github-worklog today --provider openai
github-worklog today --provider gemini
github-worklog today --provider ollama

# Use a different model
github-worklog today --provider claude --claude-model claude-sonnet-5
github-worklog today --provider ollama --ollama-model llama3.2:3b
github-worklog today --provider openai --openai-model gpt-4.1-nano
github-worklog today --provider gemini --gemini-model gemini-2.5-pro

# Debug what is happening
github-worklog today --preview --verbose
```

Every flag can also be set as an environment variable (see `.env.example`); flags win.

## Free, Local Summaries with Ollama

The default provider is Claude, but the whole pipeline runs for free and offline with [Ollama](https://ollama.com):

```bash
brew install --cask ollama      # or download the app from ollama.com
just ollama-pull                # Pulls OLLAMA_MODEL from .env
```

Then in `.env`:

```bash
SUMMARIZER_PROVIDER=ollama
OLLAMA_MODEL=gemma4:e4b
```

Models tried on an M-series Mac with 16 GB, summarising the same day:

| Model | Download | Time | Notes |
| --- | --- | --- | --- |
| `gemma4:e4b` | 9.6 GB | ~20 s | Most accurate, honours repo grouping and duplicate counts. Recommended. |
| `gemma3:4b` | 3.3 GB | ~8 s | Accurate, slightly wordier. Good on smaller machines. |
| `llama3.2:3b` | 2.0 GB | ~7 s | Fast but occasionally misattributes a PR to the wrong repo. |

Thinking-style models (gemma4, qwen3, deepseek-r1) are supported; their reasoning is switched off and stripped from the output.

You do not need to keep Ollama running: the scheduled job starts a temporary server if none is reachable and stops it again afterwards. If `ANTHROPIC_API_KEY` is also set, Ollama failures fall back to Claude; otherwise the recap is written as a plain commit list.

## Automated Daily Runs (macOS)

```bash
just install-scheduler    # Install launchd job (runs at midnight for the previous day)
just scheduler-status     # Check if scheduler is running
just logs                 # View scheduler logs
just cron                 # Run the scheduled job by hand (yesterday)
just cron --preview       # Same, without writing the file
just cron -d 2026-03-18   # Same, for a specific date
just uninstall-scheduler  # Remove the scheduler
```

If the Mac is asleep at midnight, launchd runs the job on the next wake. Re-run `just install-scheduler` after upgrading so the launchd job picks up script changes.

## Output Format

```markdown
**09/01/26**

#### `hundredrabbits/Orca`
- Built MIDI clock sync for Orca → Ableton bridge, fixed drift issues on long sessions
- Added grain density controls to the generative ambient patch

#### `block/vinyl-catalog`
- Pushed dark mode tweaks for the vinyl catalog app

---

**08/01/26**

#### `block/modular-synth`
- Shipped modular synth patch manager with Eurorack preset sharing

#### `block/tape-emulation`
- Fixed audio buffer underruns in the lo-fi tape emulation plugin
```
