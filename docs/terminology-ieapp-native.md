# IEapp 独自の用語体系提案 / IEapp-Native Terminology Proposals

**作成日**: 2026年2月2日  
**目的**: IEapp の本質から生まれた、他サービスを模倣しない独自の用語体系

---

## 🎯 IEapp の本質 / IEapp's DNA

### 3つの核心原理
1. **Local-First** - ユーザーがデータを完全にコントロール
2. **AI-Native** - AI エージェントとの協働が前提
3. **Structure-from-Text** - Markdown から構造を自動抽出

### 独自の技術的特徴
- **Markdown as Database** - Markdown を書けば、データベースになる
- **Iceberg Tables** - Apache Iceberg でデータを管理
- **MCP Protocol** - AI との対話のための専用プロトコル
- **fsspec/OpenDAL** - どこにでも保存できる柔軟性

### 他サービスとの違い
- Notion/Trello: SaaS ベース → IEapp: Local-First
- Obsidian: テキストのみ → IEapp: 構造化データベース
- Airtable: データベース優先 → IEapp: Markdown 優先

---

## 🌟 提案1: Structure-First System（構造優先）

**コンセプト**: IEapp の「Markdown から構造を抽出する」という本質に基づく命名

```
Workspace → Space (スペース)
Class → Schema (スキーマ)
Note → Record (レコード)
Attachment → Resource (リソース)
Field → Property (プロパティ)
Link → Relation (関係)
Revision → Snapshot (スナップショット)
```

### 哲学 / Philosophy
「Markdown を書けば、それが構造化される」という IEapp の核心を表現。
- Schema: 構造の定義
- Record: 構造化されたデータ
- Property: 構造の要素
- Relation: 構造間の関係

### メンタルモデル
```
Space には Schema がある
Schema は構造を定義する
Markdown を書くと Record になる
Record は Property を持つ
Record は Resource を参照する
Record 間に Relation を作る
```

### 評価
- ✅ **IEapp らしさ**: 5/5 - 構造化が中心概念
- ✅ **わかりやすさ**: 4/5 - やや技術的だが明確
- ✅ **独自性**: 4/5 - 一般的だが組み合わせは独自
- ⚠️ **親しみやすさ**: 3/5 - やや堅い

---

## 🌟 提案2: Entry-Based System（エントリー基盤）

**コンセプト**: Entry を中心に、他の用語も統一的に再設計

```
Workspace → Space (スペース)
Class → Form (フォーム)
Note → Entry (エントリー)
Attachment → Asset (アセット)
Field → Field (フィールド)
Link → Link (リンク)
Revision → Version (バージョン)
```

### 哲学 / Philosophy
「情報を記入（Entry）する」という行為を中心に据える。
- Form: 記入する形式
- Entry: 記入された内容
- Field: 記入する欄
- Asset: 添付する素材

### メンタルモデル
```
Space に Form を用意する
Form に従って Entry を書く
Entry に Field を記入する
Entry に Asset を添付する
Entry 同士を Link する
```

### 評価
- ✅ **IEapp らしさ**: 4/5 - Markdown 記入の行為を表現
- ✅ **わかりやすさ**: 5/5 - 直感的
- ⚠️ **独自性**: 3/5 - Entry は一般的
- ✅ **親しみやすさ**: 5/5 - 親しみやすい

---

## 🌟 提案3: Knowledge-First System（知識優先）

**コンセプト**: IEapp を「知識管理システム」として捉える独自の命名

```
Workspace → Space (スペース)
Class → Pattern (パターン)
Note → Knowledge (ナレッジ)
Attachment → Material (マテリアル)
Field → Aspect (側面)
Link → Connection (接続)
Revision → Version (バージョン)
```

### 哲学 / Philosophy
「知識を蓄積・構造化する」という IEapp の目的を直接表現。
- Pattern: 知識の型（会議、タスクなど）
- Knowledge: 個別の知識
- Aspect: 知識の側面（日付、参加者など）
- Material: 知識を支える素材

### メンタルモデル
```
Space で知識を管理する
Pattern で知識の型を定義する
Knowledge を蓄積する
Knowledge は Aspect を持つ
Material で知識を補完する
Connection で知識を繋げる
```

### 評価
- ✅ **IEapp らしさ**: 5/5 - 知識管理の本質
- ⚠️ **わかりやすさ**: 3/5 - Knowledge は長い
- ✅ **独自性**: 5/5 - 非常に独自
- ⚠️ **親しみやすさ**: 3/5 - やや抽象的

---

## 🌟 提案4: Flow-Based System（フロー基盤）

**コンセプト**: 情報の「流れ」を表現する動的な命名

```
Workspace → Space (スペース)
Class → Shape (シェイプ)
Note → Flow (フロー)
Attachment → Media (メディア)
Field → Facet (ファセット)
Link → Bridge (ブリッジ)
Revision → Stream (ストリーム)
```

### 哲学 / Philosophy
情報は静的ではなく、流れ（Flow）として捉える。
- Shape: 流れの形
- Flow: 情報の流れ
- Facet: 流れの切り口
- Bridge: 流れ同士を繋ぐ橋
- Stream: 流れの履歴

### メンタルモデル
```
Space で情報が流れる
Shape で流れの形を決める
Flow として情報を書く
Facet で流れを切り取る
Media が流れに乗る
Bridge で流れを繋ぐ
```

### 評価
- ⚠️ **IEapp らしさ**: 3/5 - やや詩的
- ⚠️ **わかりやすさ**: 3/5 - 比喩的
- ✅ **独自性**: 5/5 - 非常にユニーク
- ⚠️ **親しみやすさ**: 3/5 - 人を選ぶ

---

## 🌟 提案5: Unit-Based System（ユニット基盤）

**コンセプト**: 情報の「単位（Unit）」という中立的な概念

```
Workspace → Space (スペース)
Class → Format (フォーマット)
Note → Unit (ユニット)
Attachment → File (ファイル)
Field → Field (フィールド)
Link → Link (リンク)
Revision → Version (バージョン)
```

### 哲学 / Philosophy
情報を「単位（Unit）」として扱う。最小限の比喩で最大限の柔軟性。
- Format: 単位の書式
- Unit: 情報の単位
- Field: 単位の要素
- File: 単位に添付するファイル

### メンタルモデル
```
Space に Format を定義する
Format に従って Unit を作る
Unit は Field を持つ
Unit に File を添付する
Unit 同士を Link する
```

### 評価
- ✅ **IEapp らしさ**: 4/5 - 構造化の柔軟性
- ✅ **わかりやすさ**: 4/5 - 中立的で明快
- ✅ **独自性**: 4/5 - Unit は珍しい
- ✅ **親しみやすさ**: 4/5 - 適度に親しみやすい

---

## 🌟 提案6: Mark-Based System（マーク基盤）

**コンセプト**: IEapp の「Markdown」から着想した独自の命名

```
Workspace → Space (スペース)
Class → Markup (マークアップ)
Note → Mark (マーク)
Attachment → Media (メディア)
Field → Tag (タグ)
Link → Link (リンク)
Revision → Edition (エディション)
```

### 哲学 / Philosophy
「Markdown で書く（Mark する）」という行為を中心に。
- Markup: マークの仕方
- Mark: マークされた情報
- Tag: マークの要素（H2タグなど）
- Edition: マークの版

### メンタルモデル
```
Space で Markdown を書く
Markup でマークの仕方を決める
Mark として情報を記録する
Tag で情報を分類する
Media を埋め込む
Link で繋げる
```

### 評価
- ✅ **IEapp らしさ**: 5/5 - Markdown の本質
- ⚠️ **わかりやすさ**: 3/5 - Mark は多義的
- ✅ **独自性**: 5/5 - 非常に独自
- ⚠️ **親しみやすさ**: 3/5 - やや技術的

---

## 📊 比較マトリクス / Comparison Matrix

| 提案 | IEappらしさ | わかりやすさ | 独自性 | 親しみやすさ | **合計** |
|------|-----------|------------|-------|------------|---------|
| **1: Structure** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ | **16/20** |
| **2: Entry** | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | **17/20** 🥈 |
| **3: Knowledge** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | **16/20** |
| **4: Flow** | ⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | **14/20** |
| **5: Unit** | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | **16/20** |
| **6: Mark** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | **16/20** |

---

## 💎 推奨順位 / Recommended Priority

### 第1推奨: Entry-Based System (改良版)

```
Space / Form / Entry / Asset / Field
```

**推奨理由**:
1. ✅ **最もバランスが良い** - わかりやすさと独自性のバランス
2. ✅ **Entry は直感的** - 「記入する」という行為は誰でも理解
3. ✅ **Form は独自** - Class → Form は IEapp 独自の解釈
4. ✅ **他サービスと差別化** - Trello/Notionの Card, AirtableのRecordとは異なる
5. ✅ **親しみやすい** - 非プログラマーでも理解しやすい

**改良点**:
- Class → **Form** に変更（より直感的）
- Attachment → **Asset** に変更（より専門的）

---

### 第2推奨: Unit-Based System

```
Space / Format / Unit / File / Field
```

**推奨理由**:
1. ✅ **独自性がある** - Unit は珍しい選択
2. ✅ **中立的** - 柔軟な解釈が可能
3. ✅ **わかりやすい** - 情報の「単位」は理解しやすい
4. ✅ **Format が自然** - Class → Format は論理的

---

### 第3推奨: Structure-First System

```
Space / Schema / Record / Resource / Property
```

**推奨理由**:
1. ✅ **IEapp の本質** - 構造化がコンセプト
2. ✅ **技術的に正確** - データベース用語として整合
3. ✅ **Record は明確** - データの行として自然

---

## 🎨 各提案の詳細比較

### 提案1: Structure-First
**長所**: IEapp の「Structure-from-Text」を直接表現  
**短所**: やや技術的  
**適用シーン**: 開発者・データアナリスト向け

### 提案2: Entry-Based (改良版) ⭐
**長所**: 最もバランスが良く、親しみやすい  
**短所**: Entry は一般的（ただし Form との組み合わせで独自性）  
**適用シーン**: 幅広いユーザー層

### 提案3: Knowledge-First
**長所**: 知識管理の本質を表現、非常に独自  
**短所**: Knowledge は長く、抽象的  
**適用シーン**: 知識労働者・研究者向け

### 提案4: Flow-Based
**長所**: 動的で詩的、非常にユニーク  
**短所**: 比喩的すぎる可能性  
**適用シーン**: クリエイティブユーザー向け

### 提案5: Unit-Based
**長所**: 中立的で柔軟、独自性もある  
**短所**: やや地味  
**適用シーン**: バランス重視

### 提案6: Mark-Based
**長所**: Markdown の本質、非常に独自  
**短所**: Mark は多義的  
**適用シーン**: Markdown ヘビーユーザー向け

---

## 💡 推奨する最終案

### Entry-Based System (改良版)

```
Workspace → Space (スペース)
Class → Form (フォーム)
Note → Entry (エントリー)
Attachment → Asset (アセット)
Field → Field (フィールド)
Link → Link (リンク)
Revision → Version (バージョン)
```

### なぜこれが最適か？

1. **Form が独自かつ直感的**
   - Class（プログラミング用語）を避ける
   - Form（フォーム、形式）は誰でも理解できる
   - 「Form に Entry を記入する」という自然な関係

2. **Entry が親しみやすい**
   - ブログエントリー、ログエントリーで馴染み深い
   - 「記入する」という行為が直感的
   - データベース行としても自然

3. **Asset が専門的**
   - Attachment より洗練された印象
   - Entry（構造化）と Asset（非構造化）の対比が明確

4. **他サービスとの差別化**
   - Trello/Notion: Card/Board → IEapp: Entry/Form
   - Airtable: Record → IEapp: Entry
   - 独自のアイデンティティを確立

### 使用例

```
🎯 ユースケース1: 会議メモ
"Meeting Form の Entry を作成して、
 音声 Asset を添付し、
 前回の Entry に Link する"

🎯 ユースケース2: タスク管理
"Task Form の Entry を作成して、
 関連ドキュメント Asset を添付する"

🎯 ユースケース3: 人物管理
"Person Form の Entry を作成して、
 プロフィール写真 Asset を添付する"
```

**自然さ**: ⭐⭐⭐⭐⭐

---

## 🔧 実装への影響

### API 変更例

```
現在:
  /workspaces/{id}/classes
  /workspaces/{id}/notes
  /workspaces/{id}/attachments

Entry-Based 採用後:
  /spaces/{id}/forms
  /spaces/{id}/entries
  /spaces/{id}/assets
```

### TypeScript 型定義例

```typescript
interface Entry {
  id: string;
  title: string;
  form: string;  // formerly "class"
  fields: Record<string, any>;
  assets: Asset[];
  links: Link[];
  version: string;  // formerly "revision_id"
}

interface Form {
  name: string;
  fields: FormField[];
}

interface Asset {
  id: string;
  name: string;
  path: string;
}
```

---

## 📝 代替案の組み合わせ

もし Entry-Based でも他サービスを連想する場合：

### 代替A: Unit-Based
```
Space / Format / Unit / File
```
- より中立的
- 独自性がある

### 代替B: Structure-First
```
Space / Schema / Record / Resource
```
- より技術的
- データベースツールとしての性質を強調

### 代替C: Knowledge-First
```
Space / Pattern / Knowledge / Material
```
- 最も独自
- 知識管理ツールとしての性質を強調

---

## 🚀 次のステップ

1. **Entry-Based (改良版) を試す**
   - 実際のユースケースで使用感を確認
   - Form/Entry/Asset の組み合わせが自然か検証

2. **代替案を比較**
   - Unit-Based も検討
   - Knowledge-First も面白い選択肢

3. **UI での表現を確認**
   - 日本語での自然さ
   - 英語での自然さ

4. **最終決定**

---

**作成者**: GitHub Copilot AI Agent  
**最終更新**: 2026年2月2日  
**ステータス**: IEapp 独自の提案完了
