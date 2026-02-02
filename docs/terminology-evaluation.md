# 用語変更の評価：Note → object、Attachment → asset

**評価日**: 2026年2月2日  
**評価対象**: リポジトリ全体の用語統一性と変更提案の影響分析

## 📋 エグゼクティブサマリー

### 提案された変更
- `Note` → `object`
- `Attachment` → `asset`

### 結論
**❌ 推奨しない** - 以下の理由により、現在の用語を維持することを強く推奨します：

1. **「object」は意味的に曖昧** - プログラミング用語として汎用的すぎる
2. **「Class」との概念的衝突** - インスタンス-テンプレート関係が不明確になる
3. **高い変更コスト** - 1000以上の箇所への影響、公開API契約の破壊
4. **現在の用語は明確** - 一貫性があり、文脈で意味が明確

---

## 📊 現在の用語使用状況

### 1. "Note" - 主要なコンテンツエンティティ

**概念**: IEappにおける知識の基本単位；構造化されたプロパティを持つMarkdownドキュメント

**使用範囲**:
- **API エンドポイント**: `/workspaces/{id}/notes/{note_id}` (GET, PUT, DELETE, POST)
- **フロントエンド**: 34ファイル（routes, components, stores, types）
- **バックエンド**: 14ファイル（endpoints, models, tests）
- **CLI**: 10ファイル（indexer, workspace, integrity）
- **コア**: Rust実装（NoteContent, NoteMeta）
- **ドキュメント**: 仕様書、READMEで一貫して使用

**データ構造**:
```typescript
// フロントエンド (types.ts)
interface Note {
  id: string;
  title?: string;
  content: string;
  class?: string;           // Classシステムとの関連
  tags?: string[];
  links?: NoteLink[];
  attachments?: Attachment[];  // Attachmentとの関連
  revision_id: string;      // 版管理との関連
  // ...
}

interface NoteRecord {  // インデックス/検索用の軽量版
  id: string;
  title: string;
  class?: string;
  properties: Record<string, unknown>;
  // ...
}
```

**意味関係**:
```
Workspace (分離境界)
  └─ Classes (テンプレート/スキーマ)
       └─ Notes (型付きインスタンス)
            ├─ Attachments への参照
            ├─ 他の Notes への Links
            └─ Revisions (版履歴)
```

**一貫性**: ✅ 全コンポーネントで統一された使用

---

### 2. "Attachment" - バイナリアセット

**概念**: ワークスペース内に保存され、Notesから参照可能なバイナリファイル

**使用範囲**:
- **API エンドポイント**: `/workspaces/{id}/attachments` (POST, GET, DELETE)
- **フロントエンド**: 14ファイル（uploader, store, routes）
- **バックエンド**: 2ファイル（attachment endpoint）
- **CLI**: 2ファイル（attachments.py, tests）
- **ストレージ**: `{workspace}/attachments/` ディレクトリに独立管理

**データ構造**:
```typescript
interface Attachment {
  id: string;
  name: string;
  path: string;
}
```

**URI スキーム**:
```
ieapp://note/{note_id}           // Note への参照
ieapp://attachment/{attachment_id}  // Attachment への参照
```

**削除制約**: Note から参照されている Attachment は削除不可（HTTP 409）

**一貫性**: ✅ 全コンポーネントで統一された使用

---

### 3. 他の主要用語との関係

| 用語 | 役割 | Note との関係 | Attachment との関係 |
|------|------|--------------|-------------------|
| **Class** | 型定義/テンプレート | Notes はクラスのインスタンス | なし |
| **Workspace** | 隔離境界 | Notes を含む | Attachments を含む |
| **Revision** | 版管理 | Note の履歴 | なし（Attachment は版管理なし）|
| **Link** | 参照関係 | Note 間の関連 | Note から Attachment への参照 |
| **Field** | プロパティ | Class が定義、Note が値を持つ | なし |

---

## ⚠️ 提案された変更の問題点

### 1. "object" の意味的曖昧性

**問題**:
- **汎用的すぎる**: プログラミングではあらゆるものが「object」
  ```javascript
  // JavaScript の例
  const obj = {};  // これも object
  const note = new Note();  // これも object
  ```
- **文脈での曖昧さ**: "object" だけでは何のオブジェクトか不明
- **技術用語との衝突**: JSON object, Python object, Class object など

**具体例**:
```typescript
// 現在（明確）
interface Note { ... }
const note: Note = { ... };
createNote(payload: NoteCreatePayload)

// 提案後（曖昧）
interface Object { ... }  // これは何のオブジェクト？
const object: Object = { ... };  // TypeScript組み込みのObjectと紛らわしい
createObject(payload: ObjectCreatePayload)  // 何を作成？
```

---

### 2. "Class" システムとの概念的衝突

**現在の明確な関係**:
```
Class (Meeting)              Note (インスタンス)
├─ Template: "# Meeting"    ├─ title: "Weekly Sync"
├─ Fields:                   ├─ class: "Meeting"
│   ├─ Date (date)          ├─ properties:
│   ├─ Attendees (list)     │   ├─ Date: "2025-11-29"
│   └─ ...                  │   └─ Attendees: [...]
└─ Version: 1               └─ revision_id: "..."

🔍 明確: Class はテンプレート、Note はインスタンス
```

**変更後の不明確な関係**:
```
Class (Meeting)              object (???)
├─ Template: "# Meeting"    ├─ title: "Weekly Sync"
├─ Fields:                   ├─ class: "Meeting"  ❓ objectなのにclassフィールド？
│   ├─ Date (date)          ├─ properties: ...
│   └─ ...                  └─ revision_id: "..."

❓ 不明確: "object" という名前は Class との関係を隠してしまう
        型付きインスタンスという本質が失われる
```

**用語の整合性が崩れる例**:
- 現在: "This note belongs to the Meeting class" ✅ 自然
- 変更後: "This object belongs to the Meeting class" ❓ 不自然（objectは通常classのインスタンス）

---

### 3. "asset" の不明確性

**問題**:
- **多義的**: 
  - Webアセット（CSS, JS, 画像）
  - 金融資産
  - ゲームアセット
  - デジタル資産全般
- **範囲が不明確**: テキストも asset、Note も資産の一種
- **バイナリ性が不明**: "asset" だけでは二進ファイルという性質が伝わらない

**具体例**:
```typescript
// 現在（明確）
interface Attachment {  // 添付ファイルであることが明確
  id: string;
  name: string;
  path: string;
}

// 提案後（不明確）
interface Asset {  // 何の資産？テキストも？画像も？Noteも？
  id: string;
  name: string;
  path: string;
}
```

---

### 4. 大規模な破壊的変更

**影響範囲の詳細**:

| コンポーネント | Note 使用箇所 | Attachment 使用箇所 |
|--------------|--------------|-------------------|
| フロントエンド | 34ファイル（約500箇所）| 14ファイル（約80箇所）|
| バックエンド | 14ファイル（約300箇所）| 4ファイル（約30箇所）|
| CLI | 10ファイル（約200箇所）| 2ファイル（約20箇所）|
| コア (Rust) | 推定50箇所 | 推定20箇所 |
| ドキュメント | 6ファイル | 3ファイル |
| **合計** | **約1000箇所** | **約150箇所** |

**破壊される契約**:
1. **公開APIエンドポイント**: 
   ```
   /workspaces/{id}/notes        → /workspaces/{id}/objects
   /workspaces/{id}/attachments  → /workspaces/{id}/assets
   ```
   - 既存のクライアントが動作しなくなる
   - APIバージョニングが必要

2. **MCP プロトコル URI**:
   ```
   ieapp://note/{id}        → ieapp://object/{id}
   ieapp://attachment/{id}  → ieapp://asset/{id}
   ```
   - 外部統合（AIエージェント）が破壊される
   - 既存のリンクが無効になる

3. **ストレージパス** (変更すると既存ワークスペースが壊れる):
   ```
   workspaces/{id}/attachments/  → workspaces/{id}/assets/
   ```

4. **Iceberg テーブル構造**:
   - `notes` テーブル → `objects` テーブル？
   - 既存データのマイグレーションが必要

**マイグレーションの複雑さ**:
- データベーススキーマの変更
- 既存ワークスペースの移行ツール
- 後方互換性の維持期間
- APIバージョン管理の導入
- ドキュメント全体の更新
- テストの全面的書き換え

---

## 💡 より良い代替案

### もし用語変更が必要な場合の提案

#### "Note" の代替候補：

| 候補 | 長所 | 短所 | 推奨度 |
|------|------|------|--------|
| **Document** | Markdown文書という性質が明確 | やや長い | ⭐⭐⭐⭐ |
| **Entry** | 軽量、データベース用語として自然 | 汎用的 | ⭐⭐⭐ |
| **Item** | 短い、汎用的 | 意味が薄い | ⭐⭐ |
| **Record** | インスタンスの性質が明確 | データベース的すぎる | ⭐⭐⭐ |
| **Entity** | 型付き構造を示唆 | 技術的すぎる | ⭐⭐ |
| **object** | - | 上記の問題点 | ❌ |

#### "Attachment" の代替候補：

| 候補 | 長所 | 短所 | 推奨度 |
|------|------|------|--------|
| **File** | シンプル、直接的 | 汎用的 | ⭐⭐⭐⭐ |
| **Media** | 非テキスト性を示唆 | マルチメディアに限定的 | ⭐⭐⭐ |
| **Resource** | 再利用可能な性質 | 汎用的 | ⭐⭐⭐ |
| **Binary** | バイナリファイルを明示 | 技術的すぎる | ⭐⭐ |
| **Attachment** | 既に明確 | 変更の必要性なし | ⭐⭐⭐⭐⭐ |
| **asset** | - | 上記の問題点 | ⭐ |

---

## 📋 推奨事項

### 1. 現在の用語を維持（最優先推奨）

**理由**:
- ✅ 既に一貫性がある
- ✅ 文脈で意味が明確
- ✅ 技術文書で一般的な用語
- ✅ 大規模な破壊的変更を回避
- ✅ Class システムとの関係が明確

**具体的アクション**:
- 用語集（Glossary）を作成し、各用語の定義を明確化
- ドキュメントで Note-Class の関係を強調
- 新しいコントリビューターのための用語ガイドを追加

---

### 2. もし変更が必須の場合

**段階的アプローチ**:

#### Phase 1: 内部リファクタリング (6-8週間)
1. 新しい用語を型エイリアスとして導入
   ```typescript
   // 後方互換性を保つ
   type Document = Note;  // 新しい用語
   type Note = NoteInternal;  // 既存の型
   ```
2. 内部実装を徐々に移行
3. テストを更新

#### Phase 2: API バージョニング (4-6週間)
1. 新しいAPIエンドポイントを追加
   ```
   /v2/workspaces/{id}/documents  (新)
   /v1/workspaces/{id}/notes      (非推奨、維持)
   ```
2. 両方のエンドポイントを並行稼働
3. MCP プロトコルの新バージョン

#### Phase 3: ストレージマイグレーション (8-12週間)
1. 既存ワークスペースの変換ツール作成
2. 新規ワークスペースは新形式
3. 旧形式のサポート継続（12ヶ月）

#### Phase 4: 旧バージョン廃止 (12ヶ月後)
1. 廃止の警告期間
2. ユーザー通知
3. 旧APIとストレージ形式の削除

**総コスト見積もり**: 約6-8人月の開発工数

---

### 3. 代わりに改善すべき点

用語変更の代わりに、以下を改善することでより良い結果が得られます：

#### A. ドキュメントの充実化

**作成すべきドキュメント**:
```markdown
# docs/concepts/terminology.md

## 用語集

### Note（ノート）
IEappにおける知識の基本単位。Markdown形式で記述され、
Classによって定義された構造を持つ。

**関連概念**:
- Class: Noteの型定義（テンプレート）
- Note は Class のインスタンス
- Revision: Note の版管理履歴
- Link: Note 間の関連

**例**:
Meeting クラスの Note は、Date や Attendees などの
フィールドを持つ構造化されたドキュメント。

### Attachment（添付ファイル）
Noteから参照可能なバイナリファイル（画像、音声、PDFなど）。
ワークスペースの attachments ディレクトリに保存され、
ieapp://attachment/{id} で参照される。

...
```

#### B. 概念図の追加

```
docs/concepts/data-model-diagram.md に以下を追加:

┌─────────────────────────────────────────────┐
│           Workspace (分離境界)                │
│                                             │
│  ┌───────────────┐      ┌────────────────┐ │
│  │    Classes    │      │  Attachments   │ │
│  │  (Templates)  │      │  (Binary Files)│ │
│  │               │      │                │ │
│  │ ┌─────────┐   │      │ ┌────────────┐ │ │
│  │ │ Meeting │   │      │ │ audio.m4a  │ │ │
│  │ │ Task    │   │      │ │ image.png  │ │ │
│  │ │ ...     │   │      │ └────────────┘ │ │
│  │ └─────────┘   │      └────────────────┘ │
│  └───────┬───────┘               ▲          │
│          │ defines               │          │
│          │ structure             │          │
│          ▼                       │          │
│  ┌─────────────────┐             │          │
│  │      Notes      │─────references────────┘│
│  │   (Instances)   │                        │
│  │                 │                        │
│  │ ┌─────────────┐ │                        │
│  │ │ Weekly Sync │ │                        │
│  │ │ class: Mtg  │ │                        │
│  │ │ Date: ...   │ │                        │
│  │ └──────┬──────┘ │                        │
│  │        │        │                        │
│  │        └──Links to other Notes           │
│  │                 │                        │
│  │ Each Note has:  │                        │
│  │ - Revisions     │                        │
│  │ - Tags          │                        │
│  │ - Canvas pos    │                        │
│  └─────────────────┘                        │
└─────────────────────────────────────────────┘
```

#### C. API ドキュメントの強化

`docs/spec/api/rest.md` に概念説明セクションを追加:
```markdown
## Core Concepts

### Notes vs Classes
Notes are instances of user-defined Classes. Each Note:
- Belongs to exactly one Class (via `class` field)
- Follows the structure defined by that Class
- Stores content in Markdown with H2 sections mapped to Class fields
- Maintains version history through Revisions

### Attachments
Binary files (images, audio, PDFs) that can be referenced from Notes
via `ieapp://attachment/{id}` URIs. Attachments are:
- Stored independently in the workspace attachments directory
- Reference-counted (cannot be deleted while referenced by Notes)
- Not versioned (unlike Notes)
```

---

## 🎯 結論と最終推奨

### 推奨: 現在の用語を維持

**根拠**:
1. **一貫性**: 全5コンポーネント（フロントエンド、バックエンド、CLI、コア、ドキュメント）で統一
2. **明確性**: "Note" と "Attachment" は文脈で意味が明確
3. **関係性**: Class-Note のインスタンス関係が用語から理解可能
4. **コスト**: 変更による破壊的影響が大きすぎる（1000箇所以上、公開API）
5. **代替案**: ドキュメント改善で十分な効果

### 変更が必須の場合の次善策

**もし経営/製品戦略上の理由で変更が必要な場合**:
- `Note` → `Document`（objectより明確）
- `Attachment` → `File` または維持（assetより明確）
- 上記の段階的移行プラン（Phase 1-4）を実施
- 最低12ヶ月の後方互換性維持期間

### アクションプラン（現在の用語を維持する場合）

**即座に実施可能な改善**:
1. 用語集ドキュメントの作成（2-3日）
2. 概念図の追加（1-2日）
3. API ドキュメントへの概念説明追加（1日）
4. コントリビューターガイドの更新（1日）

**総コスト**: 約1週間の作業で、用語の理解度が大幅に向上

---

## 📚 参考資料

### 分析に使用したファイル

**仕様書**:
- `docs/spec/index.md` - 用語定義
- `docs/spec/data-model/overview.md` - データモデルとクラスの説明
- `docs/spec/api/rest.md` - API契約
- `docs/spec/data-model/file-schemas.yaml` - スキーマ定義

**コード**:
- `frontend/src/lib/types.ts` - TypeScript型定義（正準）
- `backend/src/app/api/endpoints/note.py` - Note API実装
- `backend/src/app/api/endpoints/attachment.py` - Attachment API実装
- `backend/src/app/models/classes.py` - データモデル
- `ieapp-core/` - Rust コア実装

**使用統計**:
- `grep -r "Note"` - 1000以上のマッチ
- `grep -r "Attachment"` - 150以上のマッチ
- 全コンポーネントで一貫した使用を確認

---

## 📝 評価者コメント

この評価は、コードベース全体の分析に基づいています。現在の用語は：

- ✅ 技術的に正確（markdown notes, file attachments）
- ✅ 一貫性がある（全コンポーネントで統一）
- ✅ 業界標準に準拠（Notion、Obsidian、Evernote等も "note" を使用）
- ✅ 教育コストが低い（新しいコントリビューターにとって理解しやすい）

提案された "object" と "asset" は：
- ❌ 意味的に曖昧
- ❌ 既存の概念（Class）と衝突
- ❌ 変更コストが極めて高い
- ❌ 明確性の向上がない

**最終判断**: 用語変更は推奨しません。代わりに、ドキュメントの改善に注力することで、より良い結果が得られます。

---

**評価完了日**: 2026年2月2日  
**評価者**: GitHub Copilot (AI Analysis)  
**次のステップ**: この評価を基に、チームで用語の方針を決定してください。
