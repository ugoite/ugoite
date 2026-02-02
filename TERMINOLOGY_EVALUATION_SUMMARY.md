# 用語変更提案の評価結果 / Terminology Change Proposal Evaluation Results

## 🔄 更新情報 / Update Information

**初回評価 (2026-02-02 初期)**: ドキュメントモデルを前提に評価 → 変更を推奨しない  
**再評価 (2026-02-02 午後)**: Milestone 3 "Markdown as Table" 完了後のデータベース行モデルを考慮 → **変更を推奨**  
**統一的探求 (2026-02-02 最新)**: 全体の用語体系を統一的に再設計 → **5つの包括的アプローチを提案** ⭐

---

## 📋 評価サマリー / Summary

**当初の提案 / Initial Proposal**: 
- `Note` → `object`
- `Attachment` → `asset`

**最新の提案 / Latest Recommendation**: 
**包括的な用語体系の再設計**（Note/Attachmentだけでなく、全体を統一的に）

### 🏆 第1推奨: Card-Based System

```
Workspace → Space (スペース)
Class → Template (テンプレート)
Note → Card (カード)
Attachment → File (ファイル)
Link → Connection (接続)
```

### 🥈 第2推奨: Simple Universal System

```
Workspace → Space (スペース)
Class → Type (タイプ)
Note → Item (アイテム)
Attachment → File (ファイル)
Link → Link (リンク)
```

---

## 🔑 重要な背景 / Critical Context

### 要件の進化 / Evolution of Requirements

**Phase 1**: ドキュメントモデル前提での評価
**Phase 2**: Markdown as Table（データベース行モデル）への対応
**Phase 3**: **統一的で親しみやすい用語体系への昇華** ⭐

> "もう少し柔軟な思想で統一感があって、プログラマーじゃなくても親しみがある、比喩すぎて違和感のないワード"

### 新しい要件 / New Requirements

1. **柔軟な思想** - 硬直的でなく、自然な理解を促す
2. **統一感** - すべての概念が一貫したメンタルモデルで理解できる
3. **非プログラマーにも親しみやすい** - 技術用語に依存しない
4. **比喩的すぎない** - 違和感のない、直感的な用語

---

## 🎯 包括的アプローチ / Holistic Approach

### 提案する5つの用語体系

| 概念 | 現在 | 案A: Simple | 案B: Card | 案C: Page | 案D: Entry | 案E: Object |
|------|------|------------|-----------|-----------|-----------|-------------|
| Container | Workspace | Space | **Space** | Space | Space | Workspace |
| Schema | Class | Type | **Template** | Collection | Category | Type |
| Instance | Note | Item | **Card** | Page | Entry | Object |
| Binary | Attachment | File | **File** | Media | File | Asset |
| Reference | Link | Link | **Connection** | Link | Link | Relation |

### 評価スコア / Evaluation Scores

| 案 | 親しみ | 統一感 | 非技術 | 非比喩 | 特徴 | **合計** |
|----|-------|--------|-------|--------|------|---------|
| **B: Card** | 5 | 5 | 5 | 3 | 5 | **23/25** 🏆 |
| **A: Simple** | 5 | 4 | 5 | 5 | 3 | **22/25** 🥈 |
| **C: Page** | 5 | 4 | 5 | 3 | 4 | **21/25** 🥉 |
| D: Entry | 4 | 4 | 4 | 5 | 3 | 20/25 |
| E: Object | 3 | 5 | 2 | 5 | 4 | 19/25 |

---

## 📚 作成されたドキュメント / Created Documents

### 1. 初回評価レポート / Initial Evaluation Reports

- **日本語**: [`docs/terminology-evaluation.md`](docs/terminology-evaluation.md)
- **English**: [`docs/terminology-evaluation-en.md`](docs/terminology-evaluation-en.md)

内容 / Contents:
- ドキュメントモデルを前提とした評価
- Initial evaluation assuming document-centric model
- 「Note を維持」という結論
- Concluded to "maintain Note"

### 2. 再評価レポート / Reevaluation Reports

- **日本語**: [`docs/terminology-reevaluation.md`](docs/terminology-reevaluation.md) 
- **English**: [`docs/terminology-reevaluation-en.md`](docs/terminology-reevaluation-en.md)

内容 / Contents:
- Milestone 3 "Markdown as Table" のパラダイムシフトを考慮
- データベース行モデルでの再評価
- **「用語変更を推奨」**という更新結論（record/object/entry）

### 3. 統一的探求レポート / Holistic Exploration ⭐ NEW

- **詳細版**: [`docs/terminology-holistic-exploration.md`](docs/terminology-holistic-exploration.md)
- **クイック比較**: [`docs/terminology-quick-comparison.md`](docs/terminology-quick-comparison.md)

内容 / Contents:
- **5つの包括的な用語体系**を提案
- Note/Attachment だけでなく、Workspace, Class, Link なども含む全体設計
- 各体系のメンタルモデルと評価
- **推奨: Card-Based System** (Space/Template/Card/File/Connection)
- 代替: Simple System (Space/Type/Item/File)

### 4. 用語ガイド / Terminology Guide

- **Path**: [`docs/concepts/terminology.md`](docs/concepts/terminology.md)

内容 / Contents:
- 全主要概念の定義（Workspace, Class, Note, Attachment, Revision, Link, Field）
- Definitions of all core concepts
- 関係図とデータモデル
- Relationship diagrams and data model
- 比較表、FAQ
- Comparison tables, FAQ

---

## 🔍 重要な発見 / Key Findings

### データベース行モデルの文脈で / In Database Row Model Context

| 用語 / Term | 技術的正確さ / Technical Accuracy | キャッチーさ / Catchiness | 推奨度 / Rating |
|------------|--------------------------------|-------------------------|----------------|
| **record** | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐⭐⭐ |
| **object** | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| **entry** | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |
| **row** | ⭐⭐⭐⭐⭐ | ⭐ | ⭐⭐ (退屈 / boring) |
| **note** | ⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐ (ドキュメントモデルと混同 / confusing with document model) |

### "object" の妥当性（データベース行として）/ Validity of "object" (as database row)

**肯定的側面 / Positive aspects**:
- ✅ ORM (Object-Relational Mapping) での標準的な用語
- ✅ Standard term in ORM (Object-Relational Mapping)
- ✅ Class のインスタンスとして自然
- ✅ Natural as instance of Class
- ✅ キャッチーでモダン
- ✅ Catchy and modern
- ✅ ビジネスオブジェクト、ドメインオブジェクトの概念と整合
- ✅ Consistent with business object, domain object concepts

**否定的側面 / Negative aspects**:
- ⚠️ JavaScript/TypeScript の組み込み Object との名前衝突
- ⚠️ Name collision with built-in Object in JavaScript/TypeScript
- ⚠️ 回避可能（namespace、型エイリアス）
- ⚠️ Avoidable (namespace, type alias)

### "asset" の妥当性（非構造化データとして）/ Validity of "asset" (as unstructured data)

**構造化データ vs 非構造化データの対比 / Structured vs Unstructured data contrast**:

| 側面 / Aspect | Object (構造化 / structured) | Asset (非構造化 / unstructured) |
|--------------|-----------------------------|---------------------------------|
| ストレージ / Storage | Iceberg テーブル / Iceberg tables | ファイルシステム / Filesystem |
| 構造 / Structure | Class定義の列 / Class-defined columns | バイナリ blob / Binary blob |
| クエリ / Query | SQL可能 / SQL-able | メタデータのみ / Metadata only |

**結論 / Conclusion**: Object-Asset の対比は明確で妥当 / Object-Asset contrast is clear and valid ✅

---

## 💡 推奨アクション / Recommended Actions

### オプション 1: "record" + "asset" を採用（最も安全）/ Option 1: Adopt "record" + "asset" (Safest)

**推奨度 / Rating**: ⭐⭐⭐⭐⭐

```
Note → Record
Attachment → Asset
```

**理由 / Reasons**:
- ✅ データベース行として技術的に最も正確
- ✅ Most technically accurate as database row
- ✅ Notion、Airtable も使用（業界標準）
- ✅ Used by Notion, Airtable (industry standard)
- ✅ Class-Record の関係が明確
- ✅ Clear Class-Record relationship
- ✅ Record-Asset の対比が自然
- ✅ Natural Record-Asset contrast

**コスト / Cost**: 3-4ヶ月の段階的移行 / 3-4 month phased migration

---

### オプション 2: "object" + "asset" を採用（提案者の意図に最も近い）/ Option 2: Adopt "object" + "asset" (Closest to proposer's intent)

**推奨度 / Rating**: ⭐⭐⭐⭐

```
Note → Object
Attachment → Asset
```

**理由 / Reasons**:
- ✅ **提案者の意図に最も合致** / **Best matches proposer's intent**
- ✅ キャッチーでモダン / Catchy and modern
- ✅ ORM的理解と整合 / Consistent with ORM understanding
- ✅ Object-Asset の対比が明確 / Clear Object-Asset contrast
- ⚠️ JavaScript/TypeScript との衝突（回避可能）/ Collision with JS/TS (avoidable)

**衝突回避策 / Collision avoidance**:
```typescript
// namespace を使用
namespace IEapp {
  export interface Object { /* ... */ }
}

// または型エイリアス
import { Object as IEappObject } from '@ieapp/types';
```

**コスト / Cost**: 3-4ヶ月の段階的移行 + 名前空間管理 / 3-4 month migration + namespace management

---

### オプション 3: "entry" + "asset" を採用（バランス型）/ Option 3: Adopt "entry" + "asset" (Balanced)

**推奨度 / Rating**: ⭐⭐⭐⭐

```
Note → Entry
Attachment → Asset
```

**理由 / Reasons**:
- ✅ バランスが良い / Well-balanced
- ✅ 親しみやすい / Friendly
- ✅ Entry-Asset の対比が自然 / Natural Entry-Asset contrast
- ⚠️ やや汎用的 / Somewhat generic

**コスト / Cost**: 3-4ヶ月の段階的移行 / 3-4 month phased migration

---

## 📊 影響範囲 / Impact Scope

### コード変更 / Code Changes

| コンポーネント | Note 使用箇所 | Attachment 使用箇所 |
|--------------|--------------|-------------------|
| フロントエンド / Frontend | 34ファイル（約500箇所）/ 34 files (~500 instances) | 14ファイル（約80箇所）/ 14 files (~80 instances) |
| バックエンド / Backend | 14ファイル（約300箇所）/ 14 files (~300 instances) | 4ファイル（約30箇所）/ 4 files (~30 instances) |
| CLI | 10ファイル（約200箇所）/ 10 files (~200 instances) | 2ファイル（約20箇所）/ 2 files (~20 instances) |
| コア (Rust) / Core (Rust) | 推定50箇所 / ~50 instances | 推定20箇所 / ~20 instances |
| **合計 / Total** | **約1000箇所 / ~1000 instances** | **約150箇所 / ~150 instances** |

### API契約の破壊 / Breaking API Contracts

```
現在 / Current:
  /workspaces/{id}/notes
  /workspaces/{id}/attachments
  ieapp://note/{note_id}
  ieapp://attachment/{attachment_id}

変更後 / After change:
  /workspaces/{id}/objects
  /workspaces/{id}/assets
  ieapp://object/{object_id}
  ieapp://asset/{asset_id}
```

**影響 / Impact**:
- 既存クライアントが動作しなくなる / Existing clients will break
- MCP統合が破壊される / MCP integrations will break
- 既存リンクが無効になる / Existing links become invalid

---

## 🎓 用語の使い方ガイド / Terminology Usage Guide

### 正しい使い方 / Correct Usage

```markdown
✅ "Create a new note in the Meeting class"
✅ "Upload an attachment to the workspace"
✅ "Each note is an instance of a class"
✅ "Link notes together using ieapp:// URIs"

❌ "Create a new object in the Meeting class" (曖昧 / ambiguous)
❌ "Upload an asset to the workspace" (不明確 / unclear)
```

### 概念の関係 / Conceptual Relationships

```
Workspace (分離境界 / isolation boundary)
  └─ Classes (テンプレート / templates)
       └─ Notes (インスタンス / instances)
            ├─ Attachments への参照 / references to Attachments
            ├─ 他の Notes への Links / Links to other Notes
            └─ Revisions (版履歴 / version history)
```

---

## 📞 次のステップ / Next Steps

### チームでの決定 / Team Decision

この評価を基に、以下を決定してください / Based on this evaluation, decide:

1. **現在の用語を維持するか？/ Maintain current terminology?**
   - はい → 用語ガイドを既存ドキュメントにリンク / Yes → Link terminology guide in existing docs
   - いいえ → 代替案と移行プランを検討 / No → Review alternatives and migration plan

2. **ドキュメントの改善を実施するか？/ Implement documentation improvements?**
   - 用語ガイドを README やコントリビューターガイドにリンク
   - Link terminology guide in README and contributor guide

3. **追加の概念図が必要か？/ Need additional concept diagrams?**
   - アーキテクチャドキュメントに統合
   - Integrate into architecture documentation

### フィードバック / Feedback

このような評価や改善提案があれば、issueやPRでお知らせください。
If you have feedback or improvement suggestions, please create an issue or PR.

---

## 📖 参考資料 / References

- **初回評価 / Initial Evaluation**: [`docs/terminology-evaluation.md`](docs/terminology-evaluation.md) (JP), [`docs/terminology-evaluation-en.md`](docs/terminology-evaluation-en.md) (EN)
- **再評価 / Reevaluation**: [`docs/terminology-reevaluation.md`](docs/terminology-reevaluation.md) (JP), [`docs/terminology-reevaluation-en.md`](docs/terminology-reevaluation-en.md) (EN)
- **統一的探求 / Holistic Exploration**: [`docs/terminology-holistic-exploration.md`](docs/terminology-holistic-exploration.md) ⭐ NEW
- **クイック比較 / Quick Comparison**: [`docs/terminology-quick-comparison.md`](docs/terminology-quick-comparison.md) ⭐ NEW
- **用語ガイド / Terminology Guide**: [`docs/concepts/terminology.md`](docs/concepts/terminology.md)
- **決定ガイド / Decision Guide**: [`docs/TERMINOLOGY_DECISION_GUIDE.md`](docs/TERMINOLOGY_DECISION_GUIDE.md)
- **仕様書 / Specifications**: [`docs/spec/index.md`](docs/spec/index.md)

---

## 🎯 最新の推奨 / Latest Recommendation

### 包括的アプローチ / Holistic Approach

Note/Attachment だけでなく、**すべての用語を統一的に再設計**することを推奨します。

**第1推奨: Card-Based System** ⭐⭐⭐⭐⭐

```
Workspace → Space
Class → Template
Note → Card
Attachment → File
Link → Connection
Revision → Version
```

**メンタルモデル**:
- Space に Template を定義
- Template に基づいて Card を作成
- Card に File を添付
- Card 同士を Connection で繋ぐ

**特徴**:
- ✅ 視覚的で直感的（物理的なカードのイメージ）
- ✅ 統一感がある（Space-Template-Card-Connection）
- ✅ 親しみやすい（Trello/Notion で馴染み深い）
- ✅ データベース行としても自然

**第2推奨: Simple Universal System** ⭐⭐⭐⭐

```
Workspace → Space
Class → Type
Note → Item
Attachment → File
Link → Link
Revision → Version
```

**特徴**:
- ✅ 最もシンプル
- ✅ 非技術者にも理解しやすい
- ✅ 柔軟で汎用的

詳細は [`docs/terminology-holistic-exploration.md`](docs/terminology-holistic-exploration.md) を参照してください。

---

**評価実施日 / Evaluation Date**: 2026年2月2日 / February 2, 2026  
**評価者 / Evaluator**: GitHub Copilot AI Agent  
**ステータス / Status**: ✅ 完了 - 包括的提案済み / Completed - Holistic proposals provided
