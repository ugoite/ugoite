# Entry-Based System 詳細ガイド / Entry-Based System Detailed Guide

## 🎯 推奨システム / Recommended System

```
Workspace → Space (スペース)
Class → Form (フォーム)
Note → Entry (エントリー)
Attachment → Asset (アセット)
Field → Field (フィールド)
Link → Link (リンク)
Revision → Version (バージョン)
```

---

## 💡 なぜ Entry-Based なのか？

### 1. IEapp の本質と整合

**IEapp = "Markdown を書けば、構造化される"**

```
Markdown を書く (記入する)
    ↓
Entry になる
    ↓
Form の構造に従う
    ↓
Field が抽出される
```

この「記入する」という行為を **Entry** で表現。

---

### 2. Form の独自性

**Class → Form への変更が鍵**

| 従来 | 問題点 | 新しい用語 | 利点 |
|------|--------|----------|------|
| Class | プログラミング用語 | **Form** | 誰でも理解できる |
| Class | 固い、技術的 | **Form** | 柔軟、親しみやすい |
| Class | Notion等と被る可能性 | **Form** | IEapp 独自の解釈 |

**Form の意味**:
- 形式、フォーマット
- 記入用紙、様式
- 型、パターン

→ すべて IEapp の「構造定義」に当てはまる！

---

### 3. Entry の自然さ

**Entry = 記入されたもの**

日常的な使用例:
- ブログエントリー（記事）
- ログエントリー（ログの記録）
- 辞書エントリー（項目）
- データベースエントリー（レコード）

→ 幅広い文脈で自然に理解される！

---

### 4. 他サービスとの差別化

| サービス | 用語 | イメージ |
|----------|------|---------|
| Trello | Card/Board | カンバン、付箋 |
| Notion | Page/Database | ページ、データベース |
| Airtable | Record/Table | スプレッドシート |
| **IEapp** | **Entry/Form** | **記入用紙、フォーム** |

→ 明確に異なるアイデンティティ！

---

## 📝 実際の使用例

### ユースケース1: 会議メモ

**現在の表現**:
```
"Meeting クラスの Note を作成して、
 音声 Attachment を添付"
```

**Entry-Based での表現**:
```
"Meeting Form の Entry を作成して、
 音声 Asset を添付"
```

**より自然**:
```
"Meeting フォームに記入して、
 音声ファイルを添付"
```

---

### ユースケース2: タスク管理

**Entry-Based での表現**:
```
"Task Form の Entry を作成
  ↓
Field を記入:
  • タイトル: "新機能の実装"
  • 期限: 2026-03-01
  • 担当者: Alice
  ↓
関連 Asset を添付:
  • 仕様書.pdf
  ↓
依存タスクに Link"
```

---

### ユースケース3: 研究ノート

**Entry-Based での表現**:
```
"Research Form の Entry を作成
  ↓
Field を記入:
  • テーマ: "機械学習の応用"
  • 日付: 2026-02-02
  • メモ: "実験結果を記録"
  ↓
実験データ Asset を添付:
  • data.csv
  • graph.png
  ↓
関連論文 Entry に Link"
```

---

## 🎨 視覚的表現

### 概念図

```
┌─────────────────────────────────────┐
│           Space: Personal           │
│                                     │
│  ┌───────────────────────────────┐  │
│  │        Form: Meeting          │  │
│  │  ┌─────────────────────────┐  │  │
│  │  │ Fields:                 │  │  │
│  │  │  • Date (date)         │  │  │
│  │  │  • Attendees (list)    │  │  │
│  │  │  • Notes (markdown)    │  │  │
│  │  └─────────────────────────┘  │  │
│  └───────────────────────────────┘  │
│                                     │
│  ┌───────────────────────┐          │
│  │ 📝 Entry              │          │
│  │ Weekly Team Sync      │          │
│  │ ─────────────────     │          │
│  │ Date: 2026-02-02      │          │
│  │ Attendees:            │          │
│  │   • Alice             │          │
│  │   • Bob               │          │
│  │ Notes: Discussed...   │          │
│  │                       │          │
│  │ 📎 Asset:             │          │
│  │   recording.m4a       │          │
│  │                       │          │
│  │ 🔗 Link:              │          │
│  │   → Previous meeting  │          │
│  └───────────────────────┘          │
│                                     │
└─────────────────────────────────────┘
```

---

## 🌊 情報の流れ

### 1. Form を定義する

```
Space に "Meeting" Form を作成
  ↓
Fields を定義:
  • Date (日付)
  • Attendees (参加者)
  • Notes (メモ)
```

### 2. Entry を作成する

```
Meeting Form に基づいて Entry を作成
  ↓
Markdown を書く:
  ---
  form: Meeting
  ---
  # Weekly Sync
  
  ## Date
  2026-02-02
  
  ## Attendees
  - Alice
  - Bob
  
  ## Notes
  Discussed Q1 roadmap
```

### 3. 構造が抽出される

```
IEapp が自動的に:
  ↓
Fields を抽出:
  • Date: 2026-02-02
  • Attendees: ["Alice", "Bob"]
  • Notes: "Discussed..."
  ↓
Entry として保存
```

---

## 📊 用語の関係性

```
Space
  │
  ├─ Form (複数)
  │   ├─ Field 定義
  │   └─ Entry (複数)
  │       ├─ Field 値
  │       ├─ Asset (添付)
  │       ├─ Link (関連)
  │       └─ Version (履歴)
  │
  └─ Asset (共有プール)
```

---

## 🔤 用語の意味

### Space (スペース)
- **意味**: 空間、場所
- **役割**: すべての情報を管理する境界
- **英語**: "Personal Space", "Project Space"
- **日本語**: "個人スペース", "プロジェクトスペース"

### Form (フォーム)
- **意味**: 形式、様式、記入用紙
- **役割**: Entry の構造を定義
- **英語**: "Meeting Form", "Task Form"
- **日本語**: "会議フォーム", "タスクフォーム"

### Entry (エントリー)
- **意味**: 記入、項目、エントリー
- **役割**: 記入された情報の単位
- **英語**: "Create an entry", "Update the entry"
- **日本語**: "エントリーを作成", "エントリーを更新"

### Asset (アセット)
- **意味**: 資産、素材
- **役割**: Entry に添付されるバイナリファイル
- **英語**: "Attach an asset", "Download asset"
- **日本語**: "アセットを添付", "アセットをダウンロード"

### Field (フィールド)
- **意味**: 欄、項目、フィールド
- **役割**: Entry の要素
- **英語**: "Fill in the field", "Required field"
- **日本語**: "フィールドに記入", "必須フィールド"

### Link (リンク)
- **意味**: 繋がり、リンク
- **役割**: Entry 間の関係
- **英語**: "Create a link", "Follow the link"
- **日本語**: "リンクを作成", "リンクをたどる"

### Version (バージョン)
- **意味**: 版、バージョン
- **役割**: Entry の履歴
- **英語**: "View version history", "Restore version"
- **日本語**: "バージョン履歴を表示", "バージョンを復元"

---

## 💭 よくある質問

### Q1: Form は「フォーム」と「形式」のどちらの意味？

**A**: 両方です！
- UI では「フォーム」として認識（記入用紙）
- 技術的には「形式」「型」として機能
- この二重性が Entry との相性を良くしている

### Q2: Entry は他のサービスでも使われているのでは？

**A**: はい、でも**Form との組み合わせが独自**です：
- WordPress: Post（記事）
- Notion: Page（ページ）
- Airtable: Record（レコード）
- **IEapp: Entry（記入）** with **Form（様式）**

### Q3: Asset は Attachment より良い？

**A**: はい、以下の理由で：
- より洗練された印象
- 「資産」という永続的なイメージ
- Entry（構造化）と Asset（非構造化）の対比が明確

### Q4: Field は変更しないの？

**A**: Field は以下の理由で維持：
- すでに広く理解されている
- 代替案（Property, Aspect）も一長一短
- Form/Entry との組み合わせで十分差別化

---

## 🚀 実装イメージ

### API エンドポイント

```
GET    /spaces/{id}/forms
POST   /spaces/{id}/forms
GET    /spaces/{id}/forms/{form_name}

GET    /spaces/{id}/entries
POST   /spaces/{id}/entries
GET    /spaces/{id}/entries/{entry_id}
PUT    /spaces/{id}/entries/{entry_id}

GET    /spaces/{id}/assets
POST   /spaces/{id}/assets
GET    /spaces/{id}/assets/{asset_id}
```

### UI での表現

```tsx
// FormEditor.tsx
function FormEditor({ formName }: { formName: string }) {
  return (
    <div>
      <h1>Edit Form: {formName}</h1>
      <FieldList fields={fields} />
    </div>
  );
}

// EntryEditor.tsx
function EntryEditor({ entryId }: { entryId: string }) {
  return (
    <div>
      <h1>Edit Entry</h1>
      <FormFields form={entry.form} />
      <AssetUploader />
      <LinkSelector />
    </div>
  );
}
```

---

## ✅ 採用時のチェックリスト

Entry-Based System を採用する場合：

- [ ] Form が「記入用紙」として自然に聞こえるか確認
- [ ] Entry が「記入されたもの」として違和感ないか確認
- [ ] Asset が Attachment より洗練されているか確認
- [ ] 日本語での使用感を確認（「フォームにエントリーを記入」）
- [ ] 英語での使用感を確認（"Create an entry in the form"）
- [ ] UI での表現を確認
- [ ] ドキュメントでの説明を確認

---

## 📝 結論

**Entry-Based System (Form/Entry/Asset)** は：

1. ✅ IEapp の「Markdown 記入 → 構造化」という本質と整合
2. ✅ Form が独自かつ直感的
3. ✅ Entry が親しみやすく、広く理解される
4. ✅ 他サービスと明確に差別化
5. ✅ 非プログラマーにも理解しやすい

**最も IEapp らしい用語体系**と言えます！

---

**推奨**: Entry-Based System を採用 ✅  
**代替**: Unit-Based System も検討価値あり  
**次のステップ**: 実際のユースケースで試用
