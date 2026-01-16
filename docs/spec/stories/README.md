# User Stories

This directory contains user stories organized by category.

## Files

- [core.yaml](core.yaml) - Essential user scenarios that define the core product
- [advanced.yaml](advanced.yaml) - Power user and experimental features
- [experimental.yaml](experimental.yaml) - Future features under consideration

## Story Format

Stories follow a structured YAML format for machine readability:

```yaml
stories:
  - id: STORY-001
    title: Short descriptive title
    type: Category (AI Native, Class, Freedom, etc.)
    as_a: Role (user, power user, AI agent, etc.)
    i_want: Desired action or capability
    so_that: Business value or outcome
    acceptance_criteria:
      - Specific, testable criteria
      - Each criterion maps to tests
    related_apis:
      - API endpoints involved
    requirements:
      - REQ-* IDs that implement this story
```

## Story Categories

| Type | Description |
|------|-------------|
| AI Native | Features leveraging AI and code execution |
| Class | Structured data and class features |
| Freedom | User control and data portability |
| Versioning | History and time travel features |
| BYOAI | Bring Your Own AI integrations |
| Live Code | Interactive code execution |

## Traceability

Stories link to:
- **Requirements**: `REQ-*` identifiers in `requirements/`
- **APIs**: REST and MCP endpoints in `api/`
- **Tests**: Acceptance criteria map to test cases

## Usage

Stories are used to:
1. Guide feature development priorities
2. Generate acceptance test templates
3. Document product capabilities
4. Communicate with stakeholders

## Verification

Tests in `docs/tests/test_stories.py` verify:
- All stories have valid format
- Referenced requirements exist
- Acceptance criteria are testable
