# 用語変更提案の評価結果 / Terminology Change Proposal Evaluation Results

## 📋 評価サマリー / Summary

**提案内容 / Proposal**: 
- `Note` → `object`
- `Attachment` → `asset`

**評価結果 / Result**: ❌ **推奨しない / NOT RECOMMENDED**

---

## 🎯 主要な結論 / Key Conclusions

### 日本語

現在の用語（"Note" と "Attachment"）を**維持することを強く推奨します**。

**理由**:
1. ✅ **一貫性**: 全5コンポーネント（フロントエンド、バックエンド、CLI、コア、ドキュメント）で統一
2. ✅ **明確性**: 文脈で意味が明確で、業界標準に準拠
3. ❌ **"object" の問題**: 意味的に曖昧で、Class システムと概念的に衝突
4. ❌ **"asset" の問題**: 多義的で、バイナリファイルという性質が不明確
5. ⚠️ **変更コスト**: 1000箇所以上の変更、公開API契約の破壊、マイグレーションが必要

### English

We **strongly recommend maintaining** the current terminology ("Note" and "Attachment").

**Reasons**:
1. ✅ **Consistency**: Unified across all 5 components (frontend, backend, CLI, core, docs)
2. ✅ **Clarity**: Clear in context and follows industry standards
3. ❌ **"object" issues**: Semantically ambiguous and conflicts conceptually with Class system
4. ❌ **"asset" issues**: Polysemous and doesn't clearly convey binary file nature
5. ⚠️ **Change cost**: 1000+ locations affected, breaks public API contracts, requires migration

---

## 📚 作成されたドキュメント / Created Documents

### 1. 詳細評価レポート / Detailed Evaluation Reports

- **日本語**: [`docs/terminology-evaluation.md`](docs/terminology-evaluation.md)
- **English**: [`docs/terminology-evaluation-en.md`](docs/terminology-evaluation-en.md)

内容 / Contents:
- 現在の用語使用状況の詳細分析
- 提案された変更の問題点
- より良い代替案
- 段階的移行プラン（変更が必須の場合）
- コスト見積もり

### 2. 用語ガイド / Terminology Guide

- **Path**: [`docs/concepts/terminology.md`](docs/concepts/terminology.md)

内容 / Contents:
- 全主要概念の定義（Workspace, Class, Note, Attachment, Revision, Link, Field）
- 関係図とデータモデル
- 比較表
- FAQ
- ベストプラクティス

---

## 🔍 重要な発見 / Key Findings

### 現在の用語は優れている / Current Terminology is Excellent

| 側面 | 評価 | 詳細 |
|------|------|------|
| 一貫性 | ⭐⭐⭐⭐⭐ | 全コンポーネントで統一 |
| 明確性 | ⭐⭐⭐⭐⭐ | 文脈で意味が明確 |
| 業界標準 | ⭐⭐⭐⭐⭐ | Notion、Obsidian等と同じ |
| 学習コスト | ⭐⭐⭐⭐⭐ | 新しいコントリビューターにも分かりやすい |

### 提案された用語の問題 / Issues with Proposed Terms

#### "object" の問題 / Issues with "object"

```typescript
// 現在（明確）/ Current (clear)
interface Note { ... }
const note: Note = { ... };

// 提案後（曖昧）/ Proposed (ambiguous)
interface Object { ... }  // 何のオブジェクト？/ What kind of object?
const object: Object = { ... };  // TypeScriptの組み込みObjectと紛らわしい / Conflicts with built-in Object
```

**主な問題 / Main Issues**:
- プログラミング用語として汎用的すぎる / Too generic as programming term
- Class との関係が不明確になる / Obscures relationship with Class
- 技術用語と衝突 / Conflicts with technical terms

#### "asset" の問題 / Issues with "asset"

**多義的 / Polysemous**:
- Web アセット / Web assets (CSS, JS, images)
- 金融資産 / Financial assets
- ゲームアセット / Game assets
- デジタル資産全般 / Digital assets in general

**不明確性 / Ambiguity**:
- バイナリファイルという性質が伝わらない / Doesn't convey binary file nature
- Note も資産の一種では？/ Aren't Notes also a type of asset?

---

## 💡 推奨アクション / Recommended Actions

### オプション 1: 現在の用語を維持（推奨）/ Option 1: Maintain Current Terms (Recommended)

**即座に実施可能な改善 / Immediate Improvements**:
1. ✅ 用語ガイドを作成済み / Terminology guide created
2. ✅ 評価レポートを作成済み / Evaluation reports created
3. ✅ 概念図を追加済み / Concept diagrams added
4. 📝 既存ドキュメントへのリンク追加を検討 / Consider adding links to existing docs

**コスト / Cost**: 1週間以内に完了済み / Completed within 1 week

**効果 / Benefits**:
- 破壊的変更なし / No breaking changes
- コードベースの安定性維持 / Maintains codebase stability
- 用語の理解度向上 / Improves terminology understanding

---

### オプション 2: 変更が必須の場合 / Option 2: If Change is Mandatory

**より良い代替案 / Better Alternatives**:

#### Note の代替候補 / Alternatives for Note

| 候補 | 推奨度 | 理由 |
|------|--------|------|
| Document | ⭐⭐⭐⭐ | Markdown文書という性質が明確 / Clear Markdown document nature |
| Entry | ⭐⭐⭐ | 軽量で自然 / Lightweight and natural |
| Record | ⭐⭐⭐ | インスタンスの性質が明確 / Clear instance nature |
| object | ❌ | 上記の問題点 / Issues described above |

#### Attachment の代替候補 / Alternatives for Attachment

| 候補 | 推奨度 | 理由 |
|------|--------|------|
| File | ⭐⭐⭐⭐ | シンプルで直接的 / Simple and direct |
| Resource | ⭐⭐⭐ | 再利用可能な性質 / Suggests reusable nature |
| Attachment | ⭐⭐⭐⭐⭐ | 既に明確 / Already clear |
| asset | ⭐ | 上記の問題点 / Issues described above |

**段階的移行プラン / Phased Migration Plan**:
1. Phase 1: 内部リファクタリング（6-8週間）/ Internal refactoring (6-8 weeks)
2. Phase 2: APIバージョニング（4-6週間）/ API versioning (4-6 weeks)
3. Phase 3: ストレージマイグレーション（8-12週間）/ Storage migration (8-12 weeks)
4. Phase 4: 旧バージョン廃止（12ヶ月後）/ Deprecate old version (after 12 months)

**総コスト見積もり / Total Cost Estimate**: 約6-8人月 / ~6-8 person-months

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

- **評価レポート（日本語）/ Evaluation Report (Japanese)**: [`docs/terminology-evaluation.md`](docs/terminology-evaluation.md)
- **評価レポート（English）/ Evaluation Report (English)**: [`docs/terminology-evaluation-en.md`](docs/terminology-evaluation-en.md)
- **用語ガイド / Terminology Guide**: [`docs/concepts/terminology.md`](docs/concepts/terminology.md)
- **仕様書 / Specifications**: [`docs/spec/index.md`](docs/spec/index.md)

---

**評価実施日 / Evaluation Date**: 2026年2月2日 / February 2, 2026  
**評価者 / Evaluator**: GitHub Copilot AI Agent  
**ステータス / Status**: ✅ 完了 / Completed
