# 用語の再評価：「Markdown as Table」アーキテクチャの観点から

**再評価日**: 2026年2月2日  
**背景**: Milestone 3 "Markdown as Table" 完了後のアーキテクチャ変更を踏まえた再検討

---

## 🎯 重要な背景情報

### アーキテクチャの根本的な変化

**従来（Milestone 1-2）**: Markdownドキュメントベース
```
Note = Markdownファイル
     → メタデータ付きドキュメント
     → Classは構造を「定義」するが、Noteは独立したファイル
```

**現在（Milestone 3以降）**: データベース行ベース  
```
Note = Icebergテーブルの1行
     → Classが定義したテーブルのレコード
     → Markdownは「再構築」される表現形式
     → 本質はデータベース行
```

この変化は、**データモデルのパラダイムシフト**です：
- 🔄 Document-centric → **Row-centric**
- 🔄 File storage → **Table storage** (Iceberg)
- 🔄 Markdown as source → **Markdown as view**

---

## 💡 提案者の意図の理解

### なぜ "object" を提案したのか

> 「markdown as table を進めることで、もはや今のnoteがドキュメントに限らず、データベースの1行になると思う。rowじゃ退屈だし専門用語すぎるし、キャッチーかなと思ってobjectにしてみた」

**提案の論理**:
1. Note は今やデータベースの「行（row）」
2. "row" は退屈で技術的すぎる
3. もっとキャッチーな用語が欲しい
4. → "object" を提案

**これは合理的な考え方です** ✅

---

## 🔍 データベース用語の比較

データベースの「行」を表す用語の選択肢：

| 用語 | 技術的正確さ | キャッチーさ | 一般理解度 | 評価 |
|------|------------|------------|-----------|------|
| **row** | ⭐⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐ | 退屈、技術的すぎる |
| **record** | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐⭐ | 正確だが固い |
| **entry** | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | バランス良い |
| **item** | ⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | 汎用的 |
| **object** | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ | キャッチーだが曖昧 |
| **document** | ⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | 旧モデルと混同 |

---

## 🎭 データベース文脈での "object" の評価

### データベースにおける "object" の使われ方

**肯定的な例**:
1. **Object-Relational Mapping (ORM)**: データベース行をオブジェクトとしてマップ
   ```python
   # Django ORM
   note = Note.objects.get(id=123)  # 行を "object" として扱う
   ```

2. **Object Database**: オブジェクト指向データベース
   ```
   オブジェクトデータベースでは、行は確かに "object"
   ```

3. **Business Object**: ビジネスロジック層での呼び方
   ```
   ビジネスアプリケーションでは、データベース行を "business object" と呼ぶ
   ```

**否定的な側面**:
1. **NoSQL の "document" との混同**:
   ```
   MongoDB: document (JSON object)
   IEapp: object (table row) 
   → 混乱の可能性
   ```

2. **プログラミング言語の object との衝突**:
   ```javascript
   const obj = {};  // JavaScript object
   const note = new Object();  // これも object
   ```

---

## 🆚 "object" vs 他の選択肢（データベース行の文脈で）

### Option A: "object" を採用

**長所**:
- ✅ キャッチー、モダン
- ✅ ORM的な理解と整合（行をオブジェクトとして扱う）
- ✅ Class のインスタンスという意味が通る
- ✅ プログラマブルな印象（API-first）

**短所**:
- ⚠️ JavaScript/TypeScript の object と名前衝突
- ⚠️ NoSQL document との混同リスク
- ⚠️ 既存のコードベースで1000箇所の変更

**推奨ケース**: 
- アプリケーションがAPI-first、プログラマブル重視
- ユーザーが開発者層
- ORM的な理解が前提

---

### Option B: "record" を採用

**長所**:
- ✅ データベース行として技術的に正確
- ✅ 「記録」という一般的な意味も持つ
- ✅ Airtable、Notion databases も "record" を使用
- ✅ Class と Record の関係が明確（型とインスタンス）

**短所**:
- ⚠️ やや固い印象
- ⚠️ "row" ほどではないが、技術的

**推奨ケース**:
- データベースとしての性質を明確にしたい
- ノーコード/ローコードツールとの親和性
- 正確さ重視

---

### Option C: "entry" を採用

**長所**:
- ✅ データベース行の一般的な呼び方
- ✅ 「エントリー」は日本語でも理解しやすい
- ✅ キャッチーで親しみやすい
- ✅ ブログエントリー、ログエントリーなど一般的
- ✅ 技術的すぎず、カジュアルすぎず

**短所**:
- ⚠️ やや汎用的（何の entry？）

**推奨ケース**:
- バランス重視
- 幅広いユーザー層
- テーブル/データベースの行という理解

---

### Option D: "item" を採用

**長所**:
- ✅ 最も汎用的で理解しやすい
- ✅ リスト、コレクションのアイテムとして自然
- ✅ DynamoDB、SharePoint も "item" を使用
- ✅ 非技術者にも分かりやすい

**短所**:
- ⚠️ 意味が薄い（何でも item になる）
- ⚠️ Class との関係性が弱い

**推奨ケース**:
- シンプルさ最優先
- 非技術者ユーザーが多い

---

## 🎯 推奨：データベース行モデルの文脈での評価

### 結論の更新

前回の評価では「Note を維持」を推奨しましたが、**Markdown as Table のパラダイムシフト**を考慮すると：

**推奨順位（高→低）**:

1. **record** ⭐⭐⭐⭐⭐
   - データベース行として正確
   - Notion、Airtable も使用
   - Class-Record の関係が明確
   - 技術的だが許容範囲

2. **object** ⭐⭐⭐⭐
   - 提案者の意図に最も近い
   - ORM的理解と整合
   - キャッチー
   - ただし、衝突リスクあり

3. **entry** ⭐⭐⭐⭐
   - バランスが良い
   - 親しみやすい
   - データベース用語としても使われる

4. **item** ⭐⭐⭐
   - シンプル
   - ただし意味が薄い

5. **note** ⭐⭐
   - 従来モデルとの継続性
   - ただし「ドキュメント」の印象が残る
   - 行ベースモデルとのミスマッチ

---

## 💭 "object" を採用する場合の戦略

もし "object" を採用するなら、以下の戦略で衝突を回避：

### 1. 名前空間の明確化

```typescript
// TypeScript での衝突回避
import { IEappObject } from './types';  // 明示的に区別
type NoteObject = IEappObject;  // エイリアス

// または
namespace IEapp {
  export interface Object {
    // IEappのObject
  }
}
```

### 2. ドキュメントでの明確な定義

```markdown
## IEapp Object

**定義**: Classによって定義されたIcebergテーブルの1行（レコード）。
Markdownとして再構築可能なデータ。

**注意**: プログラミング言語の汎用的な "object" とは異なり、
IEappでは特定の意味を持つドメイン用語です。
```

### 3. API での一貫した使用

```
/workspaces/{id}/objects/{object_id}
ieapp://object/{object_id}
```

### 4. Class との関係を強調

```
Class = テーブルスキーマ
Object = テーブルの行（Class のインスタンス）

Meeting Class → Meeting Object
Task Class → Task Object
```

---

## 📊 "attachment" → "asset" の再評価

Attachment についても、データベース行モデルの文脈で再考：

### 現在の理解

Attachment は：
- バイナリファイル（Iceberg テーブルの外）
- Object から参照される
- 別のストレージ領域に保存

### "asset" の評価（データベース文脈）

**文脈を考慮すると**:
- Object = データベース行（構造化データ）
- Asset = 外部参照（非構造化データ）

この対比は**理にかなっています** ✅

| 側面 | object (構造化) | asset (非構造化) |
|------|----------------|-----------------|
| ストレージ | Iceberg テーブル | ファイルシステム |
| 構造 | Class定義の列 | バイナリ blob |
| クエリ | SQL可能 | メタデータのみ |
| 関係 | 行と行 | 行とファイル |

**推奨**: Attachment → **Asset** は妥当 ⭐⭐⭐⭐

理由：
- Object（構造化）と Asset（非構造化）の対比が明確
- リソース的な性質を表現
- データベース外のストレージであることが明確

---

## 🎨 代替案：ハイブリッドアプローチ

### Option: 両方の用語を使い分ける

**概念レイヤーごとに用語を変える**:

```
【ユーザーインターフェース層】
  → "note" を継続使用
  → ユーザーにとってはMarkdownドキュメント

【API/データモデル層】
  → "object" または "record"
  → 開発者にとってはデータベース行

【ドキュメント】
  → 両方を説明
  → "Note は Iceberg テーブルの Object として保存される"
```

**実装例**:
```typescript
// UI コンポーネント
function NoteEditor({ note }: { note: Note }) {
  // ユーザーには "note" として見える
}

// APIクライアント
async function getObject(id: string): Promise<IEappObject> {
  // 内部的には "object"
  return api.get(`/objects/${id}`);
}

// 型エイリアス
type Note = IEappObject;  // 互換性維持
```

**長所**:
- ✅ 既存ユーザーの混乱を最小化
- ✅ 技術的には正確な用語を使用
- ✅ 段階的な移行が可能

**短所**:
- ⚠️ 2つの用語を管理する複雑さ
- ⚠️ ドキュメントが煩雑になる

---

## 🚀 最終推奨（データベース行モデルの文脈）

### 推奨 1: "record" + "asset" の組み合わせ

```
Note → Record
Attachment → Asset
```

**理由**:
- ✅ データベース行として技術的に正確
- ✅ Notion、Airtable との整合性
- ✅ Record-Asset の対比が明確
- ✅ Class-Record の関係が自然
- ⚠️ やや固い印象（許容範囲）

**適用シーン**: データベース/ローコードツールとしての性質を強調したい場合

---

### 推奨 2: "object" + "asset" の組み合わせ（提案者の意図に最も近い）

```
Note → Object
Attachment → Asset
```

**理由**:
- ✅ キャッチーでモダン
- ✅ Object-Asset の対比が明確
- ✅ 提案者の意図に合致
- ✅ ORM的理解と整合
- ⚠️ JavaScript/TypeScript との衝突（回避可能）

**適用シーン**: API-first、プログラマブル重視、開発者ユーザーが多い場合

**衝突回避策**:
```typescript
// 1. 型エイリアスで明確化
import { Object as IEappObject } from '@ieapp/types';

// 2. namespace を使用
namespace IEapp {
  export interface Object { /* ... */ }
}

// 3. ドキュメントで明確に区別
```

---

### 推奨 3: "entry" + "asset" の組み合わせ

```
Note → Entry
Attachment → Asset
```

**理由**:
- ✅ バランスが良い
- ✅ 親しみやすい
- ✅ 技術的すぎない
- ✅ Entry-Asset の対比が自然
- ⚠️ やや汎用的

**適用シーン**: 幅広いユーザー層、バランス重視

---

## 📝 実装への影響（"object" を採用する場合）

### 変更が必要な箇所（推定）

| コンポーネント | Note → Object | Attachment → Asset | 優先度 |
|--------------|--------------|-------------------|--------|
| API エンドポイント | `/workspaces/{id}/objects` | `/workspaces/{id}/assets` | 🔴 高 |
| TypeScript 型定義 | `interface Object` | `interface Asset` | 🔴 高 |
| React コンポーネント | `NoteEditor` → `ObjectEditor` | `AttachmentUploader` → `AssetUploader` | 🟡 中 |
| Python バックエンド | `note.py` → `object.py` | `attachment.py` → `asset.py` | 🔴 高 |
| Rust コア | `NoteContent` → `ObjectContent` | `AttachmentInfo` → `AssetInfo` | 🔴 高 |
| ドキュメント | 全面更新 | 全面更新 | 🟡 中 |
| MCP プロトコル | `ieapp://object/{id}` | `ieapp://asset/{id}` | 🔴 高 |

### 段階的移行プラン（"object" 採用時）

**Phase 1: 型レベルでの移行（2-3週間）**
```typescript
// 型エイリアスで互換性維持
type Object = Note;  // 新しい名前
type Note = Object;  // 後方互換

// 新しいコードは Object を使用
function createObject(data: ObjectData): Object { /* ... */ }

// 既存のコードは動作し続ける
function createNote(data: NoteData): Note { /* ... */ }
```

**Phase 2: API v2 エンドポイント追加（3-4週間）**
```
/v2/workspaces/{id}/objects  (新)
/v1/workspaces/{id}/notes    (維持)
```

**Phase 3: UI コンポーネント移行（4-6週間）**
```tsx
// 段階的にコンポーネント名を変更
ObjectList  (新)
NoteList    (deprecated)
```

**Phase 4: ドキュメント更新（2週間）**
```markdown
# IEapp では "Object" を使用
Object = Iceberg テーブルの行
（以前は "Note" と呼ばれていました）
```

**総期間**: 約3-4ヶ月

---

## 🎯 最終結論

### データベース行モデルを考慮した結論

**前回の評価**: Note を維持（ドキュメントモデル前提）  
**今回の評価**: **変更を推奨**（データベース行モデルを踏まえて）

### 推奨する組み合わせ（優先順）

1. **"record" + "asset"** ⭐⭐⭐⭐⭐
   - 最も正確で、業界標準に準拠
   - データベースツールとしての性質を明確化
   - リスクが最も低い

2. **"object" + "asset"** ⭐⭐⭐⭐
   - **提案者の意図に最も近い** ✅
   - キャッチーで、ORM的理解と整合
   - 衝突は回避可能
   - モダンな印象

3. **"entry" + "asset"** ⭐⭐⭐⭐
   - バランス型
   - 親しみやすい

### "object" を採用する場合の条件

以下の条件を満たせば、**"object" 採用を推奨**:

1. ✅ TypeScript での名前空間管理（衝突回避）
2. ✅ ドキュメントでの明確な定義
3. ✅ 3-4ヶ月の移行期間を確保
4. ✅ API バージョニングの実施
5. ✅ Class-Object 関係の明確化

### "attachment" → "asset" について

**強く推奨** ⭐⭐⭐⭐⭐

理由：
- Object（構造化データ）と Asset（非構造化データ）の対比が明確
- データベース外のリソースという性質を正確に表現
- 「添付」という動作ではなく「資産」という存在を強調

---

## 📋 次のアクション

### チームでの決定事項

1. **用語の選択**:
   - [ ] record + asset を採用
   - [ ] object + asset を採用
   - [ ] entry + asset を採用
   - [ ] note を維持（asset のみ変更）

2. **移行戦略**:
   - [ ] 段階的移行（API v2）
   - [ ] 一括移行（Breaking change）
   - [ ] ハイブリッド（UI は note、API は object）

3. **タイムライン**:
   - [ ] 3-4ヶ月の移行期間を確保
   - [ ] 即座に変更（新規プロジェクトのため）

### 推奨の実装順序（"object" 採用時）

1. ✅ この再評価ドキュメントをレビュー
2. 📝 正式な用語を決定
3. 📝 移行計画を詳細化
4. 🔧 Phase 1: 型定義の更新
5. 🔧 Phase 2: API エンドポイント追加
6. 🔧 Phase 3: コンポーネント移行
7. 📚 Phase 4: ドキュメント更新
8. ✅ Phase 5: 旧バージョン廃止

---

**再評価実施日**: 2026年2月2日  
**評価者**: GitHub Copilot AI Agent  
**ステータス**: ✅ 更新完了  
**次のステップ**: チームでの最終決定
