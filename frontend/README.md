# Ugoite Frontend

A SolidJS-based frontend for Ugoite - your AI-native, programmable knowledge base.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                    SolidStart App                        │
├─────────────────────────────────────────────────────────┤
│  routes/                                                 │
│  ├── index.tsx       Landing page                        │
│  └── entries.tsx     Main app (orchestrates components)  │
├─────────────────────────────────────────────────────────┤
│  components/         (Pure UI - no business logic)       │
│  ├── EntryList.tsx   Display entries, emit selection     │
│  ├── MarkdownEditor  Edit content, emit changes          │
│  ├── CanvasPlaceholder  Visual canvas preview            │
│  └── Nav.tsx         Navigation bar                      │
├─────────────────────────────────────────────────────────┤
│  lib/                (Business logic & state)            │
│  ├── store.ts        Reactive state management           │
│  ├── client.ts       Typed API client                    │
│  ├── api.ts          Low-level fetch utilities           │
│  └── types.ts        TypeScript interfaces               │
└─────────────────────────────────────────────────────────┘
```

## Component Responsibility Boundaries

### 🎯 Design Principle: Single Responsibility

Each component has ONE clear responsibility:

| Component | Responsibility | Accepts | Emits |
|-----------|---------------|---------|-------|
| `EntryList` | Display entries | `entries`, `loading`, `error` (Accessors) | `onSelect(entryId)` |
| `MarkdownEditor` | Edit markdown | `content`, `isDirty` | `onChange(content)`, `onSave()` |
| `CanvasPlaceholder` | Canvas preview | `entries[]` | `onSelect(entryId)` |
| `spaces/[space_id]/entries.tsx` | Orchestration | - | Coordinates all components |

### 📐 State Management Rules

```typescript
// ✅ CORRECT: Route owns state, passes to components
// routes/spaces/[space_id]/entries.tsx
const store = createEntryStore(spaceId);
<EntryList
  entries={store.entries}    // Accessor
  loading={store.loading}    // Accessor
  error={store.error}        // Accessor
  onSelect={handleSelect}
/>

// ❌ WRONG: Component creates its own store
// components/EntryList.tsx
const store = createEntryStore(...);  // NO! Violates responsibility
```

### Controlled vs Standalone Mode

`EntryList` supports two modes:
1. **Controlled**: Receives state from parent (recommended for routes)
2. **Standalone**: Creates internal store (for isolated usage/testing)

```typescript
// Controlled mode (used in routes)
<EntryList entries={store.entries} loading={store.loading} error={store.error} />

// Standalone mode (self-contained)
<EntryList spaceId="my-space" />
```

## Features (Milestone 5)

- **Entry List View**: Browse and manage entries in a sidebar
- **Markdown Editor**: Edit entries with live preview and Cmd/Ctrl+S save
- **Structured Properties**: H2 headers are automatically extracted as properties
- **Optimistic Updates**: UI updates immediately, reconciles with server
- **Canvas Placeholder**: Preview of the infinite canvas feature (Story 4)
- **View Toggle**: Switch between List and Canvas views

## Getting Started

### Prerequisites

- For contributor-managed tool versions, use the repository root `mise.toml`
  via `mise run setup`.
- Backend service running (see backend README) if you are not using the root
  `mise run dev` workflow.

### Installation

```bash
mise run //frontend:install
```

### Development

For the canonical auth-aware contributor workflow that starts backend,
frontend, and docsite together, return to the repository root and run
`mise run dev` as described in the main [README](../README.md#setup--development-mise).

Use the command below only when you intentionally want frontend-isolated
iteration and already have a reachable local backend. If the frontend throws a
`BACKEND_URL must be set` startup error, that is your cue to go back to the
repository root and use `mise run dev` instead:

```bash
# Start the frontend-only dev server
mise run //frontend:dev
```

### Testing

```bash
# Run unit/component tests
npm test

# Run tests once
npm run test:run
```

Important: E2E tests are located in the root `/e2e` directory and use Bun's native test runner. See the main project README for details.

### Linting & Formatting

```bash
npm run lint
npm run format
```

## Project Structure

```text
src/
├── components/       # Reusable UI components
│   ├── EntryList.tsx      # Entry list sidebar
│   ├── MarkdownEditor.tsx # Editor with preview
│   ├── CanvasPlaceholder.tsx # Canvas view placeholder
│   └── Nav.tsx            # Navigation bar
├── lib/             # Business logic & API
│   ├── api.ts            # API fetch utilities
│   ├── client.ts         # Typed API client
│   ├── store.ts          # SolidJS reactive store
│   └── types.ts          # TypeScript interfaces
├── routes/          # Layout routes and leaf pages
│   ├── index.tsx         # Public landing page
│   ├── login.tsx         # Auth entrypoint
│   └── spaces/[space_id]/
│       ├── entries.tsx         # Layout route for the entries branch
│       ├── entries/index.tsx   # Entries list page
│       ├── forms.tsx           # Layout route for the forms branch
│       └── forms/index.tsx     # Forms list page
└── test/            # Test utilities
    ├── setup.ts          # Vitest setup
    └── mocks/            # MSW handlers
```

## Routing Conventions

SolidStart nested routes in this repository use a **parent layout + `index.tsx` leaf**
pattern.

- Put shared setup for a route branch in the parent route file such as
  `entries.tsx`, `forms.tsx`, `sql.tsx`, or `[entry_id].tsx`.
- Layout routes accept `RouteSectionProps` from `@solidjs/router` and render
  `props.children`.
- Put the actual page UI for that branch in `index.tsx`.
- Use the layout file to own shared context, loaders, and route-level state;
  keep `index.tsx` focused on the leaf page content.

Examples from the current tree:

```text
src/routes/spaces/[space_id]/entries.tsx                 # layout/context
src/routes/spaces/[space_id]/entries/index.tsx           # entries list page
src/routes/spaces/[space_id]/entries/[entry_id].tsx      # nested layout
src/routes/spaces/[space_id]/entries/[entry_id]/index.tsx # entry detail page
src/routes/spaces/[space_id]/sql.tsx                     # layout
src/routes/spaces/[space_id]/sql/index.tsx               # SQL page
```

When you add a deeper dynamic segment, keep the same rule: the dynamic segment
file is the layout for that branch, and its `index.tsx` is the leaf page for
the default view under that segment.

E2E tests are located in the root `/e2e` directory using Bun's native test runner.

## API Integration

The frontend connects to the backend REST API:

- `GET /spaces` - List spaces
- `POST /spaces` - Create space
- `GET /spaces/{id}/entries` - List entries
- `POST /spaces/{id}/entries` - Create entry
- `PUT /spaces/{id}/entries/{entryId}` - Update entry (requires `parent_revision_id`)
- `DELETE /spaces/{id}/entries/{entryId}` - Delete entry

See [docs/spec/api/rest.md](../docs/spec/api/rest.md) and [docs/spec/api/mcp.md](../docs/spec/api/mcp.md) for the API specification.

## TDD Approach

Following Milestone 5 TDD steps:
1. ✅ Component tests for entry list store with REST mocks
2. ✅ E2E smoke tests for entry creation/editing (in /e2e directory)
3. ✅ Canvas placeholder with visual baseline

## Building for Production

```bash
npm run build
npm start
```

## This project uses

- [SolidJS](https://solidjs.com) - Reactive UI framework
- [Solid Start](https://start.solidjs.com) - Meta-framework
- [TailwindCSS](https://tailwindcss.com) - Styling
- [Vitest](https://vitest.dev) - Unit testing
- [Bun Test](https://bun.sh/docs/cli/test) - E2E testing (in /e2e directory)
- [MSW](https://mswjs.io) - API mocking
