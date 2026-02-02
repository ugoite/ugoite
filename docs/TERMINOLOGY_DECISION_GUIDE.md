# 用語決定ガイド / Terminology Decision Guide

**作成日 / Date**: 2026年2月2日  
**目的 / Purpose**: チームが最終的な用語を決定するためのガイド

---

## 🎯 決定すべき事項 / Decisions to Make

### 1. 用語の選択 / Terminology Selection

以下の3つの選択肢から選んでください：

**Option A: "record" + "asset"** ⭐⭐⭐⭐⭐
```
Note → Record
Attachment → Asset
```
- 最も技術的に正確
- Notion、Airtable と同じ
- リスクが最も低い

**Option B: "object" + "asset"** ⭐⭐⭐⭐ (提案者の意図に最も近い)
```
Note → Object
Attachment → Asset
```
- キャッチー、モダン
- ORM的理解と整合
- JavaScript衝突は回避可能

**Option C: "entry" + "asset"** ⭐⭐⭐⭐
```
Note → Entry
Attachment → Asset
```
- バランスが良い
- 親しみやすい

---

## 🤔 選択の判断基準 / Selection Criteria

### Option A ("record") を選ぶべき場合

✅ **以下に当てはまる場合に推奨**:
- データベース/ローコードツールとしての性質を強調したい
- 技術的正確さを最優先
- Notion、Airtable との親和性を重視
- リスクを最小化したい
- ビジネスユーザー、データアナリストがメインユーザー

❌ **以下の場合は避けるべき**:
- "record" がかた苦しいと感じる
- もっとキャッチーな用語が欲しい

**想定ユーザー層**:
- ビジネスユーザー
- データアナリスト
- プロダクトマネージャー
- ノーコード/ローコードツール経験者

---

### Option B ("object") を選ぶべき場合

✅ **以下に当てはまる場合に推奨**:
- API-first、プログラマブル重視
- ORM的な理解を前提とできる
- キャッチーさ、モダンさを重視
- 提案者の意図（row より catchy）に最も近い
- 開発者がメインユーザー

⚠️ **以下の条件を満たす必要あり**:
- TypeScript で namespace または型エイリアスを使用
- ドキュメントで明確に定義
- JavaScript の Object との違いを説明

❌ **以下の場合は避けるべき**:
- JavaScript/TypeScript の Object との混同を避けたい
- より正確な用語を使いたい

**想定ユーザー層**:
- ソフトウェア開発者
- API 統合担当者
- DevOps エンジニア
- ORM 経験者

---

### Option C ("entry") を選ぶべき場合

✅ **以下に当てはまる場合に推奨**:
- 技術者と非技術者の両方がユーザー
- バランスの良い用語が欲しい
- 親しみやすさを重視
- 幅広いユーザー層

❌ **以下の場合は避けるべき**:
- より技術的に正確な用語が欲しい
- より独自性のある用語が欲しい

**想定ユーザー層**:
- 混成チーム（開発者 + ビジネス）
- 研究者、学生
- 個人ユーザー
- ブログ、Wiki 経験者

---

## 📊 比較マトリクス / Comparison Matrix

| 基準 / Criteria | record | object | entry |
|----------------|--------|--------|-------|
| **技術的正確さ** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |
| **キャッチーさ** | ⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ |
| **業界標準** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |
| **学習容易さ** | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| **衝突リスク** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| **ORM理解** | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |
| **ユーザー層の広さ** | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |

---

## 🎨 ブランディングとの関係 / Relationship with Branding

### IEapp のポジショニング / IEapp Positioning

IEapp を以下のどれとして位置づけますか？

**A. データベース/ローコードツール**
```
例: Airtable, Notion Database, Google Sheets
→ "record" が最適
```

**B. プログラマブル知識ベース**
```
例: Obsidian + API, Roam Research API
→ "object" が最適
```

**C. ユニバーサル知識管理**
```
例: Notion, Confluence
→ "entry" が最適
```

---

## 🚀 実装への影響 / Implementation Impact

### すべてのオプションで共通の変更 / Common Changes for All Options

| 変更箇所 / Area | 影響度 / Impact | 工数 / Effort |
|----------------|----------------|--------------|
| API エンドポイント | 🔴 高 | 2-3週間 |
| TypeScript 型定義 | 🔴 高 | 1-2週間 |
| React コンポーネント | 🟡 中 | 3-4週間 |
| Python バックエンド | 🔴 高 | 2-3週間 |
| Rust コア | 🔴 高 | 3-4週間 |
| ドキュメント | 🟡 中 | 2週間 |
| MCP プロトコル | 🔴 高 | 1週間 |

**総期間 / Total Duration**: 3-4ヶ月

### オプション固有の追加作業 / Option-Specific Additional Work

**"object" を選んだ場合の追加作業**:
- [ ] TypeScript namespace 設計（1週間）
- [ ] JavaScript Object との違いをドキュメント化（3日）
- [ ] IDE 自動補完での区別テスト（2日）

**追加期間**: +2週間

---

## 📝 決定プロセス / Decision Process

### ステップ 1: ステークホルダーの特定

- [ ] プロダクトオーナー
- [ ] 技術リード
- [ ] UX デザイナー
- [ ] ドキュメント担当
- [ ] メインユーザー代表

### ステップ 2: 投票または合意形成

**方法 1: 投票**
```
各ステークホルダーが第1希望、第2希望を選択
最も票が集まったものを採用
```

**方法 2: 合意形成**
```
全員が納得するまで議論
最終的に1つに絞る
```

### ステップ 3: 決定の記録

決定した用語を以下に記録：

```
決定日: ____年__月__日

採用する用語:
- Note → ____________
- Attachment → asset

理由:
______________________________________
______________________________________
______________________________________

決定者:
- __________________
- __________________
- __________________
```

### ステップ 4: 移行計画の作成

- [ ] Phase 1: 型定義の更新（__週間）
- [ ] Phase 2: API v2 エンドポイント（__週間）
- [ ] Phase 3: UI コンポーネント（__週間）
- [ ] Phase 4: ドキュメント更新（__週間）

---

## 🎯 推奨フロー / Recommended Flow

```
1. このガイドを読む（30分）
   ↓
2. チームミーティングを開く（1-2時間）
   - 各オプションの長所短所を議論
   - IEapp のポジショニングを確認
   - ユーザー層を明確化
   ↓
3. 投票または合意（30分）
   ↓
4. 決定を記録（10分）
   ↓
5. 移行計画を作成（1時間）
   ↓
6. 実装開始
```

---

## 📞 質問と懸念事項 / Questions and Concerns

### よくある質問 / FAQ

**Q1: "object" と JavaScript の Object は本当に区別できる？**

A: はい。以下の方法で区別可能：
```typescript
// 方法1: namespace
namespace IEapp {
  export interface Object { /* ... */ }
}
const obj: IEapp.Object = { /* ... */ };

// 方法2: 型エイリアス
import { Object as IEappObject } from '@ieapp/types';
```

**Q2: Attachment → Asset の変更は必須？**

A: はい、強く推奨します。理由：
- Object（構造化）と Asset（非構造化）の対比が明確
- データベース外のリソースという性質を正確に表現
- 単独で変更しても価値がある

**Q3: 移行期間中、両方の用語を使える？**

A: はい。API v1 と v2 を並行稼働させることで可能：
```
/v1/workspaces/{id}/notes      (旧)
/v2/workspaces/{id}/objects    (新)
```

**Q4: ドキュメントはすぐに更新する必要がある？**

A: いいえ。段階的に更新可能：
```
Phase 1: 型定義 → 開発者向けドキュメントのみ更新
Phase 2: API → API リファレンスのみ更新
Phase 3: UI → ユーザー向けドキュメント更新
```

---

## ✅ 決定後のチェックリスト / Post-Decision Checklist

決定後、以下を確認：

### 即座に実施
- [ ] 決定内容を README に記載
- [ ] チーム全体に通知
- [ ] GitHub Issue を作成

### 1週間以内
- [ ] 詳細な移行計画を作成
- [ ] 関連する Issue をすべてリンク
- [ ] マイルストーンを設定

### 1ヶ月以内
- [ ] Phase 1 を開始
- [ ] 進捗を週次レビュー

---

## 📚 参考資料 / References

- **初回評価**: [`docs/terminology-evaluation.md`](../terminology-evaluation.md)
- **再評価**: [`docs/terminology-reevaluation.md`](../terminology-reevaluation.md) ⭐
- **用語ガイド**: [`docs/concepts/terminology.md`](../concepts/terminology.md)
- **サマリー**: [`TERMINOLOGY_EVALUATION_SUMMARY.md`](../TERMINOLOGY_EVALUATION_SUMMARY.md)

---

**作成者**: GitHub Copilot AI Agent  
**最終更新**: 2026年2月2日  
**ステータス**: レビュー待ち
