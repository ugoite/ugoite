# Ugoite AI Agent Development Guide

## 🎯 Project Context

**Vision**: "Local-First, AI-Native Knowledge Space for the Post-SaaS Era"

**Core Principles**: Low Cost, Easy, Freedom

**Key Architecture**:
- Storage: shared Rust/OpenDAL abstraction (not traditional DB)
- AI Interface: MCP with resource-first integration
- Stack: SolidStart frontend/docsite on Deno + Rust `ugoite-server` (Axum) + Rust crates (`ugoite-core`, `ugoite-cli`, `ugoite-storage`)
- Data Model: Markdown sections as structured fields

**Documentation** (ALWAYS consult for details):
- [`README.md`](README.md) - Setup & quick start
- [`docs/spec/`](docs/spec/) - Complete specifications
- [`.github/workflows/`](.github/workflows/) - CI requirements

---

## 🛠️ Development Commands

```bash
# Setup & run
mise run setup            # Install all dependencies and pre-commit hooks
mise run dev              # Start frontend + Rust server + docsite
mise run test             # Run all tests
mise run e2e              # Run E2E tests

# Quality checks (see .github/workflows/ for exact CI commands)
cargo fmt --all --check                      # Format Rust (CI-aligned)
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets --all-features --locked
deno fmt --check
deno lint tools
deno task check
deno task test

```

---

## 📋 Task Completion Checklist

Before marking any task as complete:

- [ ] Read relevant docs in `docs/spec/` for context
- [ ] Implement with tests (TDD preferred, >80% coverage)
- [ ] All CI checks pass (see `.github/workflows/`)
- [ ] Run `mise run test` successfully
- [ ] Manual testing completed
- [ ] Code aligns with "Local-First, AI-Native" philosophy

**Critical**: Check `.github/workflows/` and run those exact commands locally. No exceptions.

---

## 🤖 Codex Skills

Repo-local Codex skills live under `.codex/skills/` and should be treated as the
preferred task-specific playbooks for this repository. Use the orientation,
implementation, validation, and release skills when they match the task, and
keep them aligned with `README.md`, `docs/spec/`, and `.github/workflows/`.

## 💡 Best Practices

- **2025 Standards**: Research current best practices before implementing new features
- **Type Safety**: Complete type hints (Python) and types (TypeScript)
- **Security**: Validate all inputs; no sandboxed code execution in Milestone 2
- **Data Privacy**: Never commit secrets; respect local-first principle

---

**Remember**: Every decision should support user freedom, data ownership, and seamless AI interaction.
