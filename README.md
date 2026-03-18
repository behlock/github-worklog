![Orca Sequencer](assets/orca-sequencer.png)
*[Orca](https://github.com/hundredrabbits/Orca) - livecoding environment (reminds me of commit log)*

# GitHub Worklog

A CLI tool that fetches your daily GitHub commits, uses an LLM to summarize them into meaningful bullet points, and maintains a running log of your work.

## Features

- Fetches all commits across all your repositories for a given day
- Summarizes commits using cloud or local models
- Maintains a single markdown file with all daily recaps
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
```

## Usage

```bash
just                      # Show all available commands
just preview              # Preview today's recap without saving
just today                # Generate today's recap
just generate 2026-03-18  # Generate for a specific date
just week                 # Generate for the past 7 days
just config               # View configuration
```

### CLI Flags

```bash
# Switch provider on the fly
github-worklog today --provider claude
github-worklog today --provider openai
github-worklog today --provider gemini
github-worklog today --provider ollama

# Use a different model
github-worklog today --provider ollama --ollama-model llama3.2:3b
github-worklog today --provider openai --openai-model gpt-4.1-nano
github-worklog today --provider gemini --gemini-model gemini-2.5-pro
```

## Automated Daily Runs (macOS)

```bash
just install-scheduler    # Install launchd job (runs daily at midnight)
just scheduler-status     # Check if scheduler is running
just logs                 # View scheduler logs
just cron                 # Run manually (generate + commit + push)
just uninstall-scheduler  # Remove the scheduler
```

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
