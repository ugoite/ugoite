# Terminology Reevaluation: Considering "Markdown as Table" Architecture

**Reevaluation Date**: February 2, 2026  
**Context**: Reconsidering terminology after Milestone 3 "Markdown as Table" completion

---

## 🎯 Critical Background Information

### Fundamental Architectural Shift

**Previous (Milestone 1-2)**: Markdown document-based
```
Note = Markdown file
     → Document with metadata
     → Class "defines" structure, but Note is independent file
```

**Current (Milestone 3+)**: Database row-based
```
Note = Row in Iceberg table
     → Record in Class-defined table
     → Markdown is "reconstructed" representation
     → Essence is database row
```

This represents a **paradigm shift** in data model:
- 🔄 Document-centric → **Row-centric**
- 🔄 File storage → **Table storage** (Iceberg)
- 🔄 Markdown as source → **Markdown as view**

---

## 💡 Understanding the Proposer's Intent

### Why "object" was proposed

> "Considering the current milestone, by advancing 'markdown as table', the current 'note' is no longer limited to documents, but becomes a row in a database. 'row' would be boring and too technical, so I thought 'object' would be catchy."

**Proposal logic**:
1. Note is now a database "row"
2. "row" is boring and too technical
3. Want a catchier term
4. → Propose "object"

**This is reasonable thinking** ✅

---

## 🔍 Comparison of Database Row Terminology

Options for representing database "row":

| Term | Technical Accuracy | Catchiness | General Understanding | Rating |
|------|-------------------|------------|---------------------|--------|
| **row** | ⭐⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐ | Boring, too technical |
| **record** | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐⭐ | Accurate but stiff |
| **entry** | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | Well-balanced |
| **item** | ⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | Generic |
| **object** | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ | Catchy but ambiguous |
| **document** | ⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | Confusion with old model |

---

## 🎭 Evaluating "object" in Database Context

### How "object" is used in databases

**Positive examples**:
1. **Object-Relational Mapping (ORM)**: Maps database rows as objects
   ```python
   # Django ORM
   note = Note.objects.get(id=123)  # Treats row as "object"
   ```

2. **Object Database**: Object-oriented databases
   ```
   In object databases, rows are indeed "objects"
   ```

3. **Business Object**: Business logic layer terminology
   ```
   Business applications call database rows "business objects"
   ```

**Negative aspects**:
1. **Confusion with NoSQL "document"**:
   ```
   MongoDB: document (JSON object)
   IEapp: object (table row)
   → Potential confusion
   ```

2. **Collision with programming language objects**:
   ```javascript
   const obj = {};  // JavaScript object
   const note = new Object();  // This is also an object
   ```

---

## 🆚 "object" vs Other Options (In Database Row Context)

### Option A: Adopt "object"

**Pros**:
- ✅ Catchy, modern
- ✅ Consistent with ORM understanding (treating rows as objects)
- ✅ Makes sense as Class instance
- ✅ Programmable impression (API-first)

**Cons**:
- ⚠️ Name collision with JavaScript/TypeScript object
- ⚠️ Risk of confusion with NoSQL document
- ⚠️ 1000+ code changes in existing codebase

**Recommended case**: 
- Application is API-first, programmable-focused
- Users are developer audience
- ORM-style understanding is assumed

---

### Option B: Adopt "record"

**Pros**:
- ✅ Technically accurate as database row
- ✅ Also has general meaning of "recording"
- ✅ Used by Airtable, Notion databases
- ✅ Clear Class-Record relationship (type and instance)

**Cons**:
- ⚠️ Somewhat stiff impression
- ⚠️ Not as technical as "row" but still technical

**Recommended case**:
- Want to clarify database nature
- Affinity with no-code/low-code tools
- Accuracy-focused

---

### Option C: Adopt "entry"

**Pros**:
- ✅ Common term for database rows
- ✅ "Entry" is easy to understand in Japanese too
- ✅ Catchy and friendly
- ✅ Common in blog entries, log entries, etc.
- ✅ Not too technical, not too casual

**Cons**:
- ⚠️ Somewhat generic (entry of what?)

**Recommended case**:
- Balance-focused
- Broad user base
- Understanding as table/database row

---

### Option D: Adopt "item"

**Pros**:
- ✅ Most generic and easy to understand
- ✅ Natural as item in list/collection
- ✅ Used by DynamoDB, SharePoint
- ✅ Easy for non-technical users

**Cons**:
- ⚠️ Weak meaning (anything can be item)
- ⚠️ Weak relationship with Class

**Recommended case**:
- Simplicity is top priority
- Many non-technical users

---

## 🎯 Recommendation: Evaluation in Database Row Model Context

### Updated Conclusion

Previous evaluation recommended "maintain Note", but considering the **Markdown as Table paradigm shift**:

**Priority ranking (high→low)**:

1. **record** ⭐⭐⭐⭐⭐
   - Accurate as database row
   - Used by Notion, Airtable
   - Clear Class-Record relationship
   - Technical but acceptable

2. **object** ⭐⭐⭐⭐
   - Closest to proposer's intent
   - Consistent with ORM understanding
   - Catchy
   - But has collision risks

3. **entry** ⭐⭐⭐⭐
   - Well-balanced
   - Friendly
   - Used as database term

4. **item** ⭐⭐⭐
   - Simple
   - But weak meaning

5. **note** ⭐⭐
   - Continuity with legacy model
   - But "document" impression remains
   - Mismatch with row-based model

---

## 💭 Strategy for Adopting "object"

If adopting "object", avoid collisions with this strategy:

### 1. Clear namespace

```typescript
// Avoid collision in TypeScript
import { IEappObject } from './types';  // Explicitly distinguish
type NoteObject = IEappObject;  // Alias

// Or use namespace
namespace IEapp {
  export interface Object {
    // IEapp's Object
  }
}
```

### 2. Clear definition in documentation

```markdown
## IEapp Object

**Definition**: A row (record) in an Iceberg table defined by a Class.
Data that can be reconstructed as Markdown.

**Note**: Unlike the generic "object" in programming languages,
in IEapp this is a domain term with specific meaning.
```

### 3. Consistent use in API

```
/workspaces/{id}/objects/{object_id}
ieapp://object/{object_id}
```

### 4. Emphasize relationship with Class

```
Class = Table schema
Object = Table row (instance of Class)

Meeting Class → Meeting Object
Task Class → Task Object
```

---

## 📊 Reevaluating "attachment" → "asset"

Reconsidering Attachment in database row model context:

### Current Understanding

Attachment is:
- Binary file (outside Iceberg tables)
- Referenced from Objects
- Stored in separate storage area

### Evaluating "asset" (database context)

**Considering context**:
- Object = database row (structured data)
- Asset = external reference (unstructured data)

This contrast **makes sense** ✅

| Aspect | object (structured) | asset (unstructured) |
|--------|--------------------|--------------------|
| Storage | Iceberg tables | Filesystem |
| Structure | Class-defined columns | Binary blob |
| Querying | SQL-able | Metadata only |
| Relationship | Row to row | Row to file |

**Recommendation**: Attachment → **Asset** is reasonable ⭐⭐⭐⭐

Reasons:
- Clear contrast between Object (structured) and Asset (unstructured)
- Expresses resource-like nature
- Clear that it's storage outside database

---

## 🎨 Alternative: Hybrid Approach

### Option: Use both terms by layer

**Different terms per conceptual layer**:

```
【User Interface Layer】
  → Continue using "note"
  → To users, it's a Markdown document

【API/Data Model Layer】
  → "object" or "record"
  → To developers, it's a database row

【Documentation】
  → Explain both
  → "Note is stored as Object in Iceberg table"
```

**Implementation example**:
```typescript
// UI components
function NoteEditor({ note }: { note: Note }) {
  // Appears as "note" to users
}

// API client
async function getObject(id: string): Promise<IEappObject> {
  // Internally "object"
  return api.get(`/objects/${id}`);
}

// Type alias
type Note = IEappObject;  // Maintain compatibility
```

**Pros**:
- ✅ Minimize confusion for existing users
- ✅ Use technically accurate terms
- ✅ Gradual migration possible

**Cons**:
- ⚠️ Complexity of managing two terms
- ⚠️ Documentation becomes verbose

---

## 🚀 Final Recommendation (Database Row Model Context)

### Recommendation 1: "record" + "asset" combination

```
Note → Record
Attachment → Asset
```

**Reasons**:
- ✅ Technically accurate as database row
- ✅ Consistent with Notion, Airtable
- ✅ Clear Record-Asset contrast
- ✅ Natural Class-Record relationship
- ⚠️ Somewhat stiff (acceptable)

**Use case**: When emphasizing nature as database/low-code tool

---

### Recommendation 2: "object" + "asset" combination (Closest to proposer's intent)

```
Note → Object
Attachment → Asset
```

**Reasons**:
- ✅ Catchy and modern
- ✅ Clear Object-Asset contrast
- ✅ Matches proposer's intent
- ✅ Consistent with ORM understanding
- ⚠️ Collision with JavaScript/TypeScript (avoidable)

**Use case**: API-first, programmable-focused, many developer users

**Collision avoidance**:
```typescript
// 1. Clarify with type alias
import { Object as IEappObject } from '@ieapp/types';

// 2. Use namespace
namespace IEapp {
  export interface Object { /* ... */ }
}

// 3. Clearly distinguish in documentation
```

---

### Recommendation 3: "entry" + "asset" combination

```
Note → Entry
Attachment → Asset
```

**Reasons**:
- ✅ Well-balanced
- ✅ Friendly
- ✅ Not too technical
- ✅ Natural Entry-Asset contrast
- ⚠️ Somewhat generic

**Use case**: Broad user base, balance-focused

---

## 📝 Implementation Impact (If adopting "object")

### Areas requiring changes (estimated)

| Component | Note → Object | Attachment → Asset | Priority |
|-----------|--------------|-------------------|----------|
| API endpoints | `/workspaces/{id}/objects` | `/workspaces/{id}/assets` | 🔴 High |
| TypeScript types | `interface Object` | `interface Asset` | 🔴 High |
| React components | `NoteEditor` → `ObjectEditor` | `AttachmentUploader` → `AssetUploader` | 🟡 Medium |
| Python backend | `note.py` → `object.py` | `attachment.py` → `asset.py` | 🔴 High |
| Rust core | `NoteContent` → `ObjectContent` | `AttachmentInfo` → `AssetInfo` | 🔴 High |
| Documentation | Complete update | Complete update | 🟡 Medium |
| MCP protocol | `ieapp://object/{id}` | `ieapp://asset/{id}` | 🔴 High |

### Phased migration plan (when adopting "object")

**Phase 1: Type-level migration (2-3 weeks)**
```typescript
// Maintain compatibility with type aliases
type Object = Note;  // New name
type Note = Object;  // Backward compatibility

// New code uses Object
function createObject(data: ObjectData): Object { /* ... */ }

// Existing code continues to work
function createNote(data: NoteData): Note { /* ... */ }
```

**Phase 2: Add API v2 endpoints (3-4 weeks)**
```
/v2/workspaces/{id}/objects  (new)
/v1/workspaces/{id}/notes    (maintain)
```

**Phase 3: Migrate UI components (4-6 weeks)**
```tsx
// Gradually rename components
ObjectList  (new)
NoteList    (deprecated)
```

**Phase 4: Update documentation (2 weeks)**
```markdown
# IEapp uses "Object"
Object = Row in Iceberg table
(Previously called "Note")
```

**Total duration**: ~3-4 months

---

## 🎯 Final Conclusion

### Conclusion considering database row model

**Previous evaluation**: Maintain Note (assuming document model)
**Current evaluation**: **Recommend change** (considering database row model)

### Recommended combinations (priority order)

1. **"record" + "asset"** ⭐⭐⭐⭐⭐
   - Most accurate, follows industry standards
   - Clarifies nature as database tool
   - Lowest risk

2. **"object" + "asset"** ⭐⭐⭐⭐
   - **Closest to proposer's intent** ✅
   - Catchy, consistent with ORM understanding
   - Collisions are avoidable
   - Modern impression

3. **"entry" + "asset"** ⭐⭐⭐⭐
   - Balance type
   - Friendly

### Conditions for adopting "object"

If the following conditions are met, **recommend adopting "object"**:

1. ✅ Namespace management in TypeScript (avoid collision)
2. ✅ Clear definition in documentation
3. ✅ Secure 3-4 month migration period
4. ✅ Implement API versioning
5. ✅ Clarify Class-Object relationship

### About "attachment" → "asset"

**Strongly recommended** ⭐⭐⭐⭐⭐

Reasons:
- Clear contrast between Object (structured data) and Asset (unstructured data)
- Accurately represents nature as resource outside database
- Emphasizes "asset" as existence rather than "attachment" as action

---

## 📋 Next Actions

### Team decisions

1. **Term selection**:
   - [ ] Adopt record + asset
   - [ ] Adopt object + asset
   - [ ] Adopt entry + asset
   - [ ] Maintain note (only change asset)

2. **Migration strategy**:
   - [ ] Phased migration (API v2)
   - [ ] Bulk migration (Breaking change)
   - [ ] Hybrid (UI uses note, API uses object)

3. **Timeline**:
   - [ ] Secure 3-4 month migration period
   - [ ] Change immediately (new project)

### Recommended implementation order (when adopting "object")

1. ✅ Review this reevaluation document
2. 📝 Decide official terminology
3. 📝 Detail migration plan
4. 🔧 Phase 1: Update type definitions
5. 🔧 Phase 2: Add API endpoints
6. 🔧 Phase 3: Migrate components
7. 📚 Phase 4: Update documentation
8. ✅ Phase 5: Deprecate old version

---

**Reevaluation Date**: February 2, 2026  
**Evaluator**: GitHub Copilot AI Agent  
**Status**: ✅ Update completed  
**Next step**: Final team decision
