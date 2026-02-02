# 用語体系クイック比較 / Quick Terminology Comparison

## 🎯 5つの提案を一目で比較

| 現在 / Current | 案A: Simple | 案B: Card | 案C: Page | 案D: Entry | 案E: Object |
|---------------|-------------|-----------|-----------|------------|-------------|
| **Workspace** | Space | **Space** | Space | Space | Workspace |
| **Class** | Type | **Template** | Collection | Category | Type |
| **Note** | Item | **Card** | Page | Entry | Object |
| **Attachment** | File | **File** | Media | File | Asset |
| **Field** | Property | Field | Property | Field | Attribute |
| **Link** | Link | **Connection** | Link | Link | Relation |
| **Revision** | Version | Version | Version | Revision | Snapshot |

---

## ⭐ 推奨: 案B "Card-Based"

```
Space / Template / Card / File / Connection
```

### なぜこれが最適か？

**1. 統一感が抜群** ✅
- Space（空間）にCardを置く → 直感的
- Template で Card の形を決める → わかりやすい
- Card に File を添付 → 自然
- Card と Card を Connection で繋ぐ → 視覚的

**2. 親しみやすい** ✅
- 「カード」は誰でも知っている
- Trello, Notion Board で馴染み深い
- プログラマーでなくても理解できる

**3. 比喩的だが違和感なし** ✅
- カードは物理世界にある → 理解しやすい
- でも押し付けがましくない → 自然
- データベース行としても成立 → 柔軟

**4. 他の用語との調和** ✅
- Template（テンプレート）→ Card の型を定義
- File（ファイル）→ Card に添付
- Connection（接続）→ Card 同士を繋ぐ
- Version（バージョン）→ Card の履歴

---

## 📝 実際の使用例

### 案B (Card) での表現

```
🎯 ユースケース1: 会議メモ
"Meeting Template の Card を作成して、
 音声 File を添付し、
 前回の会議 Card に Connection する"

🎯 ユースケース2: タスク管理
"Task Template の Card を作成して、
 関連ドキュメント File を添付し、
 依存タスク Card に Connection する"

🎯 ユースケース3: 人物管理
"Person Template の Card を作成して、
 プロフィール写真 File を添付し、
 所属プロジェクト Card に Connection する"
```

**自然さ**: ⭐⭐⭐⭐⭐

---

### 案A (Item) での表現

```
🎯 ユースケース1: 会議メモ
"Meeting Type の Item を作成して、
 音声 File を添付し、
 前回の会議 Item に Link する"
```

**自然さ**: ⭐⭐⭐⭐ (シンプルだが少し味気ない)

---

### 案C (Page) での表現

```
🎯 ユースケース1: 会議メモ
"Meeting Collection の Page を作成して、
 音声 Media を埋め込み、
 前回の会議 Page に Link する"
```

**自然さ**: ⭐⭐⭐⭐ (Notionライク、ただしPageはドキュメント的)

---

## 🎨 視覚的イメージ

### 案B: Card-Based

```
┌─────────────────────────────────┐
│        Space: Personal          │
│                                 │
│  ┌─────────────────────────┐   │
│  │  Template: Meeting      │   │
│  │  ┌─ Fields:             │   │
│  │  │  • Date (date)       │   │
│  │  │  • Attendees (list)  │   │
│  │  └─ ...                 │   │
│  └─────────────────────────┘   │
│                                 │
│  ┌───────────────────┐          │
│  │ 📇 Card           │          │
│  │ Weekly Sync       │          │
│  │ ───────────       │          │
│  │ Date: 2026-01-15  │          │
│  │ Attendees: ...    │          │
│  │                   │          │
│  │ 📎 File:          │          │
│  │   recording.m4a   │          │
│  │                   │          │
│  │ 🔗 Connection:    │          │
│  │   → Prev meeting  │          │
│  └───────────────────┘          │
└─────────────────────────────────┘
```

直感的！ ✅

---

## 💭 よくある質問

### Q1: Card は比喩的すぎない？

**A**: カードは以下の理由で違和感がありません：
- 物理世界に実在する（名刺、カード）
- データベースの「行」を表現する比喩として自然
- Trello/Notion で実績がある
- 視覚的なUI（カンバンビュー）と相性抜群

### Q2: Item の方がシンプルでは？

**A**: 確かにItemはシンプルですが：
- Itemは何でも当てはまる → 個性がない
- Cardは情報の単位として特徴的
- ただし、Cardが違和感あるなら Item も良い選択

### Q3: Page（Notionライク）じゃダメ？

**A**: Pageも良い選択肢ですが：
- Pageはドキュメントの印象が強い
- データベース行として考えるとCardの方が自然
- ただし、Notion 経験者には馴染み深い

### Q4: Template は Class より良い？

**A**: はい、以下の理由で：
- Templateはプログラミング用語でない
- 「型紙」「ひな形」として直感的
- Cardとの相性が良い（CardのTemplate）

---

## 🚀 推奨する決定フロー

```
Step 1: Card-Based を試す
  ↓
  OK? → 採用 ✅
  ↓
  NG (比喩的すぎる?)
  ↓
Step 2: Simple (Item) を試す
  ↓
  OK? → 採用 ✅
  ↓
  NG (地味すぎる?)
  ↓
Step 3: Page-Based を試す
  ↓
  採用 ✅
```

---

## 📊 最終スコア

| 案 | 親しみ | 統一感 | 非技術 | 非比喩 | 特徴 | **合計** |
|----|-------|--------|-------|--------|------|---------|
| **B: Card** | 5 | 5 | 5 | 3 | 5 | **23/25** 🏆 |
| **A: Simple** | 5 | 4 | 5 | 5 | 3 | **22/25** 🥈 |
| **C: Page** | 5 | 4 | 5 | 3 | 4 | **21/25** 🥉 |
| D: Entry | 4 | 4 | 4 | 5 | 3 | 20/25 |
| E: Object | 3 | 5 | 2 | 5 | 4 | 19/25 |

---

## ✅ 結論

**推奨**: 案B "Card-Based" システム

```
Space / Template / Card / File / Connection / Version
```

これが：
- ✅ 最も統一感がある
- ✅ 親しみやすい
- ✅ 柔軟で特徴的
- ✅ データベース行モデルとも整合

**代替**: もしCardが比喩的すぎると感じたら、案A "Simple" (Item)

---

**次のステップ**: チームで実際のユースケースを試して最終決定！
