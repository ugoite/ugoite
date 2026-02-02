# 統一的な用語体系の探求 / Holistic Terminology Exploration

**作成日**: 2026年2月2日  
**目的**: プログラマー以外にも親しみやすく、統一感があり、比喩的すぎない用語体系を探求

---

## 🎯 要件 / Requirements

### 求められる特性

1. **柔軟な思想** - 硬直的でなく、自然な理解を促す
2. **統一感** - すべての概念が一貫したメンタルモデルで理解できる
3. **非プログラマーにも親しみやすい** - 技術用語に依存しない
4. **比喩的すぎない** - 違和感のない、直感的な用語

### 現在の用語体系

```
Workspace (ワークスペース)
  └─ Class (クラス) - テーブルスキーマ
       └─ Note (ノート) - データベース行
            ├─ Field (フィールド) - 列
            ├─ Attachment (添付) - バイナリファイル
            ├─ Link (リンク) - 関連
            └─ Revision (リビジョン) - 版管理
```

**課題**:
- "Class" はプログラミング用語
- "Note" はドキュメントモデルの印象（実際は行）
- "Field" は技術的
- 全体として統一的なメンタルモデルが不明確

---

## 🌟 提案する用語体系 (5つのアプローチ)

### 案A: シンプル・ユニバーサル (Simple Universal)

**コンセプト**: 最もシンプルで普遍的な用語。技術的でもなく、比喩的でもない。

```
Workspace → Space (スペース)
Class → Type (タイプ)
Note → Item (アイテム)
Attachment → File (ファイル)
Field → Property (プロパティ)
Link → Link (リンク)
Revision → Version (バージョン)
```

**メンタルモデル**:
```
Space には Type がある
Type は Item の形を定義する
Item は Property を持つ
Item は File を参照できる
Item は他の Item に Link できる
```

**評価**:
- ✅ **親しみやすさ**: 5/5 - 誰でも理解できる
- ✅ **統一感**: 4/5 - シンプルで一貫している
- ✅ **技術的でない**: 5/5 - 完全に一般用語
- ✅ **比喩的でない**: 5/5 - 直接的な表現
- ⚠️ **特徴的**: 3/5 - シンプルすぎて個性がない

**用例**:
```
"Meeting" Type の Item を作成
"Image" File を Item に添付
Item 間を Link で接続
```

---

### 案B: カード・ベース (Card-Based)

**コンセプト**: Trello/Notionライクなカードメタファー。視覚的でわかりやすい。

```
Workspace → Board (ボード)
Class → Template (テンプレート)
Note → Card (カード)
Attachment → File (ファイル)
Field → Field (フィールド)
Link → Connection (接続)
Revision → Version (バージョン)
```

**メンタルモデル**:
```
Board には Template がある
Template は Card の型を定義する
Card は情報を持つ
Card に File を添付できる
Card 間を Connection で繋げる
```

**評価**:
- ✅ **親しみやすさ**: 5/5 - カードは誰でも理解
- ✅ **統一感**: 5/5 - ボード/カードで一貫
- ✅ **技術的でない**: 5/5 - 物理的メタファー
- ⚠️ **比喩的でない**: 3/5 - カードは比喩（ただし違和感なし）
- ✅ **特徴的**: 5/5 - 視覚的でわかりやすい

**用例**:
```
"Meeting" Template の Card を作成
Board に新しい Card を追加
Card 同士を Connection で繋ぐ
```

---

### 案C: ページ・ベース (Page-Based)

**コンセプト**: Notion/Confluenceライクなページメタファー。ドキュメント的でも、データベース的でもある。

```
Workspace → Space (スペース)
Class → Collection (コレクション)
Note → Page (ページ)
Attachment → Media (メディア)
Field → Property (プロパティ)
Link → Link (リンク)
Revision → Version (バージョン)
```

**メンタルモデル**:
```
Space には Collection がある
Collection は Page をグループ化する
Page は情報を持つ
Page に Media を埋め込める
Page 間を Link で繋げる
```

**評価**:
- ✅ **親しみやすさ**: 5/5 - ページは馴染み深い
- ✅ **統一感**: 4/5 - Space/Collection/Pageで階層的
- ✅ **技術的でない**: 5/5 - 一般用語
- ⚠️ **比喩的でない**: 3/5 - ページは比喩（ただしNotionで実績）
- ✅ **特徴的**: 4/5 - 知識管理ツールらしい

**用例**:
```
"Meeting" Collection の Page を作成
Page に Media を埋め込む
関連 Page に Link する
```

---

### 案D: エントリー・ベース (Entry-Based)

**コンセプト**: データベース的だがフレンドリー。ブログエントリーのイメージ。

```
Workspace → Space (スペース)
Class → Category (カテゴリ)
Note → Entry (エントリー)
Attachment → File (ファイル)
Field → Field (フィールド)
Link → Link (リンク)
Revision → Revision (リビジョン)
```

**メンタルモデル**:
```
Space には Category がある
Category は Entry を分類する
Entry は情報を持つ
Entry に File を添付できる
Entry 間を Link で繋げる
```

**評価**:
- ✅ **親しみやすさ**: 4/5 - エントリーは理解しやすい
- ✅ **統一感**: 4/5 - Space/Category/Entryで階層的
- ✅ **技術的でない**: 4/5 - 一般用語
- ✅ **比喩的でない**: 5/5 - 直接的
- ⚠️ **特徴的**: 3/5 - やや地味

**用例**:
```
"Meeting" Category の Entry を作成
Entry に File を添付
関連 Entry に Link
```

---

### 案E: オブジェクト・ベース（改良版）(Object-Based Refined)

**コンセプト**: データベース行を "Object" として扱うが、他の用語も統一的に見直す。

```
Workspace → Workspace (ワークスペース)
Class → Schema (スキーマ) → Type (タイプ)
Note → Object (オブジェクト)
Attachment → Asset (アセット)
Field → Attribute (属性)
Link → Relation (関係)
Revision → Snapshot (スナップショット)
```

**メンタルモデル**:
```
Workspace には Type がある
Type は Object の形を定義する
Object は Attribute を持つ
Object は Asset を参照する
Object 間を Relation で繋げる
```

**評価**:
- ⚠️ **親しみやすさ**: 3/5 - やや技術的
- ✅ **統一感**: 5/5 - データモデルとして一貫
- ⚠️ **技術的でない**: 2/5 - 技術寄り
- ✅ **比喩的でない**: 5/5 - 直接的
- ✅ **特徴的**: 4/5 - データベースツールとして明確

**用例**:
```
"Meeting" Type の Object を作成
Object に Asset を参照
Object 間を Relation で繋ぐ
```

---

## 📊 比較マトリクス

| 案 | 親しみやすさ | 統一感 | 非技術的 | 非比喩的 | 特徴的 | 総合 |
|----|------------|--------|---------|---------|--------|------|
| **A: Simple** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | **22/25** |
| **B: Card** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | **23/25** |
| **C: Page** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ | **21/25** |
| **D: Entry** | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | **20/25** |
| **E: Object** | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | **19/25** |

---

## 🎨 詳細分析

### 案A: Simple Universal の魅力

**最もバランスが良い選択肢**

**長所**:
- Item は誰でも理解できる（アイテム）
- Type/Space も一般的
- 技術者でも非技術者でも同じ理解
- データベース行としても自然（行 = アイテム）

**使用例**:
```
Space: "Personal Knowledge"
  Type: "Meeting"
    Item: "Weekly Team Sync"
      Property: Date = "2026-01-15"
      Property: Attendees = ["Alice", "Bob"]
      File: "recording.m4a"
```

**日本語での自然さ**:
- "Meeting タイプのアイテムを作る"
- "アイテムにファイルを添付"
- "アイテム同士をリンク"

**類似ツール**:
- DynamoDB: Item
- SharePoint: Item
- Airtable: Record (類似)

---

### 案B: Card-Based の魅力

**最も視覚的でわかりやすい**

**長所**:
- カードは物理的メタファーで直感的
- Board/Template/Card の階層が明確
- Trello/Notion Boardでの実績
- 視覚的UIと相性が良い

**使用例**:
```
Board: "Personal Knowledge"
  Template: "Meeting"
    Card: "Weekly Team Sync"
      Field: Date = "2026-01-15"
      Field: Attendees = ["Alice", "Bob"]
      File: "recording.m4a"
```

**日本語での自然さ**:
- "Meeting テンプレートのカードを作る"
- "カードにファイルを添付"
- "カード同士を接続"

**類似ツール**:
- Trello: Board/Card
- Notion: Board view
- Miro: Board/Card

---

### 案C: Page-Based の魅力

**Notionライクで馴染み深い**

**長所**:
- ページは誰もが理解
- ドキュメント的でもデータベース的でもある
- Notionでの実績（Notion DatabaseのPage）
- Collection は直感的な分類

**使用例**:
```
Space: "Personal Knowledge"
  Collection: "Meetings"
    Page: "Weekly Team Sync"
      Property: Date = "2026-01-15"
      Property: Attendees = ["Alice", "Bob"]
      Media: "recording.m4a"
```

**日本語での自然さ**:
- "Meetings コレクションのページを作る"
- "ページにメディアを埋め込む"
- "関連ページにリンク"

**類似ツール**:
- Notion: Space/Collection/Page
- Confluence: Space/Page
- OneNote: Notebook/Page

---

## 💡 推奨順位

### 第1推奨: **案B - Card-Based** ⭐⭐⭐⭐⭐

```
Board / Template / Card / File
```

**理由**:
1. ✅ **最も統一感がある**: Board-Template-Card-Connectionで一貫したメタファー
2. ✅ **視覚的に理解しやすい**: 物理的なカードのイメージ
3. ✅ **親しみやすい**: Trello/Notionで馴染み深い
4. ✅ **データベース行との整合**: カードは「情報の単位」として自然
5. ✅ **UI との相性**: カンバンビュー、ギャラリービューと相性抜群

**ただし**: "Board" はやや限定的。"Space" の方が柔軟かも。

**改良版**: Board → **Space** に変更
```
Space / Template / Card / File
```

これが**最もバランスが良い**！

---

### 第2推奨: **案A - Simple Universal** ⭐⭐⭐⭐

```
Space / Type / Item / File
```

**理由**:
1. ✅ **最もシンプル**: 説明不要の明快さ
2. ✅ **柔軟**: どんな文脈でも使える
3. ✅ **非技術者フレンドリー**: 専門知識不要
4. ✅ **データベースとの整合**: Item = 行として自然

**ただし**: 少し地味。個性がない。

---

### 第3推奨: **案C - Page-Based** ⭐⭐⭐⭐

```
Space / Collection / Page / Media
```

**理由**:
1. ✅ **Notion 的**: 成功事例がある
2. ✅ **ドキュメント的**: ページは馴染み深い
3. ✅ **Collection が直感的**: カテゴリより自然

**ただし**: ページはドキュメントの印象が強い（データベース行とのギャップ）

---

## 🔧 実装への影響

### 案B (Card-Based) を採用した場合

#### API 変更
```
/workspaces/{id}/spaces        (Board → Space に変更推奨)
/spaces/{id}/templates          (Class → Template)
/spaces/{id}/cards              (Note → Card)
/spaces/{id}/files              (Attachment → File)
/spaces/{id}/connections        (Link → Connection)
```

#### TypeScript 型
```typescript
interface Card {
  id: string;
  title: string;
  template: string;  // formerly "class"
  fields: Record<string, any>;
  files: File[];
  connections: Connection[];
}

interface Template {
  name: string;
  fields: TemplateField[];
}

interface File {
  id: string;
  name: string;
  path: string;
}
```

#### データモデル用語
```
Space (スペース) - 分離境界
  └─ Template (テンプレート) - スキーマ定義
       └─ Card (カード) - データの単位
            ├─ Field (フィールド) - プロパティ
            ├─ File (ファイル) - 参照ファイル
            ├─ Connection (接続) - 関連
            └─ Version (バージョン) - 履歴
```

---

## 🎯 最終推奨

### 推奨する用語体系

```
Workspace → Space (スペース)
Class → Template (テンプレート)
Note → Card (カード)
Attachment → File (ファイル)
Field → Field (フィールド)
Link → Connection (接続)
Revision → Version (バージョン)
```

**理由**:
1. ✅ **統一感**: Space-Template-Card-File-Connectionで一貫
2. ✅ **親しみやすさ**: 物理的メタファーで直感的
3. ✅ **非技術的**: プログラマー用語を排除
4. ✅ **比喩的だが違和感なし**: カードは馴染み深い比喩
5. ✅ **柔軟**: 様々な用途に対応
6. ✅ **特徴的**: IEapp の個性を表現

### メンタルモデル

```
Space には Template がある
Template は Card の型を定義する
Card は情報を表現する
Card に File を添付できる
Card 同士を Connection で繋げる
Card の Version を管理する
```

---

## 📝 代替案の組み合わせ

もし Card が比喩的すぎると感じる場合：

### 代替A: Card → Item
```
Space / Template / Item / File
```
- より中立的
- データベース的にも自然

### 代替B: Template → Type
```
Space / Type / Card / File
```
- より短くシンプル
- 型システムとの整合性

---

## 🚀 次のステップ

1. **チームでの議論**: 5つの案（特にA, B, C）を検討
2. **用例の確認**: 実際のユースケースで試す
3. **UI モックアップ**: 視覚的に確認
4. **最終決定**: 1つの体系に絞る
5. **移行計画**: 段階的な移行戦略

---

**作成者**: GitHub Copilot AI Agent  
**最終更新**: 2026年2月2日  
**ステータス**: チーム決定待ち
