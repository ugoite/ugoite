# IEapp Frontend

A SolidJS-based frontend for IEapp - your AI-native, programmable knowledge base.

## Features (Milestone 5)

- **Note List View**: Browse and manage notes in a sidebar
- **Markdown Editor**: Edit notes with live preview and Cmd/Ctrl+S save
- **Structured Properties**: H2 headers are automatically extracted as properties
- **Optimistic Updates**: UI updates immediately, reconciles with server
- **Canvas Placeholder**: Preview of the infinite canvas feature (Story 4)
- **View Toggle**: Switch between List and Canvas views

## Getting Started

### Prerequisites

- Node.js >= 22
- Backend service running (see backend README)

### Installation

```bash
npm install
```

### Development

```bash
# Set backend URL and start dev server
BACKEND_URL=http://localhost:8000 npm run dev
```

### Testing

```bash
# Run unit/component tests
npm test

# Run tests once
npm run test:run

# Run E2E tests (requires dev server)
npm run test:e2e
```

### Linting & Formatting

```bash
npm run lint
npm run format
```

## Project Structure

```
src/
├── components/       # Reusable UI components
│   ├── NoteList.tsx       # Note list sidebar
│   ├── MarkdownEditor.tsx # Editor with preview
│   ├── CanvasPlaceholder.tsx # Canvas view placeholder
│   └── Nav.tsx            # Navigation bar
├── lib/             # Business logic & API
│   ├── api.ts            # API fetch utilities
│   ├── client.ts         # Typed API client
│   ├── store.ts          # SolidJS reactive store
│   └── types.ts          # TypeScript interfaces
├── routes/          # Page components
│   ├── index.tsx         # Landing page
│   └── notes.tsx         # Main notes view
├── test/            # Test utilities
│   ├── setup.ts          # Vitest setup
│   └── mocks/            # MSW handlers
└── e2e/             # Playwright E2E tests
```

## API Integration

The frontend connects to the backend REST API:

- `GET /workspaces` - List workspaces
- `POST /workspaces` - Create workspace
- `GET /workspaces/{id}/notes` - List notes
- `POST /workspaces/{id}/notes` - Create note
- `PUT /workspaces/{id}/notes/{noteId}` - Update note (requires `parent_revision_id`)
- `DELETE /workspaces/{id}/notes/{noteId}` - Delete note

See [docs/spec/04_api_and_mcp.md](../docs/spec/04_api_and_mcp.md) for full API specification.

## TDD Approach

Following Milestone 5 TDD steps:
1. ✅ Component tests for note list store with REST mocks
2. ✅ Playwright smoke tests for note creation/editing
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
- [Playwright](https://playwright.dev) - E2E testing
- [MSW](https://mswjs.io) - API mocking
