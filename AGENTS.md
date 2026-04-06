# Ugoite AI Agent Development Guide

## 🎯 Project Context

**Vision**: "Local-First, AI-Native Knowledge Space for the Post-SaaS Era"

**Core Principles**: Low Cost, Easy, Freedom

**Key Architecture**:
- Storage: shared Rust/OpenDAL abstraction (not traditional DB)
- AI Interface: MCP with resource-first integration
- Stack: SolidStart (Bun) + FastAPI (Python 3.12+) + Rust (ugoite-core/ugoite-cli) + uv
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
mise run dev              # Start frontend + backend
mise run test             # Run all tests
mise run e2e              # Run E2E tests

# Quality checks (see .github/workflows/ for exact CI commands)
uvx ruff check --select ALL --ignore-noqa .  # Lint Python (CI-aligned)
uvx ruff format --check .                    # Format check (CI-aligned)
cd backend && uv run ty check .              # Type check backend
cd ugoite-core && uv run ty check .          # Type check core Python bindings
cd ugoite-cli && cargo fmt --check && cargo clippy --no-default-features -- -D warnings && mise run test:coverage
cd frontend && biome ci .                    # Check frontend

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

## 💡 Best Practices

- **2025 Standards**: Research current best practices before implementing new features
- **Type Safety**: Complete type hints (Python) and types (TypeScript)
- **Security**: Validate all inputs; no sandboxed code execution in Milestone 2
- **Data Privacy**: Never commit secrets; respect local-first principle

---

**Remember**: Every decision should support user freedom, data ownership, and seamless AI interaction.
