# IEapp AI Agent Development Guide

## 🎯 Project Context

**Vision**: "Local-First, AI-Native Knowledge Space for the Post-SaaS Era"

**Core Principles**: Low Cost, Easy, Freedom

**Key Architecture**:
- Storage: `fsspec`-based (not traditional DB)
- AI Interface: MCP with Wasm/JavaScript sandbox
- Stack: SolidStart (Bun) + FastAPI (Python 3.12+) + uv
- Data Model: Markdown sections as structured fields

**Documentation** (ALWAYS consult for details):
- [`README.md`](../README.md) - Setup & quick start
- [`docs/spec/`](../docs/spec/) - Complete specifications
- [`.github/workflows/`](workflows/) - CI requirements

---

## 🛠️ Development Commands

```bash
# Setup & run
mise run install          # Install all dependencies
mise run dev              # Start frontend + backend
mise run test             # Run all tests

# Quality checks (see .github/workflows/ for exact CI commands)
uvx ruff format .         # Auto-format Python
uvx ruff check --fix .    # Fix lint issues
cd backend && uv run ty check .      # Type check backend
cd ieapp-cli && uv run ty check .    # Type check CLI
cd frontend && biome ci . # Check frontend

# Sandbox
mise run sandbox:build    # Build Wasm sandbox
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
- **Security**: User code runs in Wasm sandbox only; validate all inputs
- **Data Privacy**: Never commit secrets; respect local-first principle

---

**Remember**: Every decision should support user freedom, data ownership, and seamless AI interaction.
