# セキュリティ監査レポート

作成日: 2025-11-30

## 概要

本ドキュメントは、ieappリポジトリのセキュリティ監査結果をまとめたものです。
発見された脆弱性、セキュリティ上の懸念事項、および改善提案を記載しています。

---

## 🔴 高優先度 (Critical/High)

### Issue 1: 認証・認可機能の欠如

**重要度**: 🔴 Critical

**現状**:
- APIエンドポイントに認証・認可機能がない
- すべてのユーザーがすべてのワークスペースとノートにアクセス可能
- README.mdでも「Authentication (JWT/OAuth) is not implemented yet」と明記

**影響**:
- 不正なデータアクセス
- データの改ざん・削除
- ワークスペース間のデータ漏洩

**該当ファイル**:
- `backend/src/app/api/endpoints/workspaces.py` (全エンドポイント)
- `backend/src/app/main.py`

**推奨対応**:
1. JWT認証またはOAuth2の実装
2. ワークスペースごとのアクセス制御
3. ロールベースアクセス制御 (RBAC) の導入

**コード例** (改善後):
```python
from fastapi import Depends
from fastapi.security import OAuth2PasswordBearer

oauth2_scheme = OAuth2PasswordBearer(tokenUrl="token")

async def get_current_user(token: str = Depends(oauth2_scheme)):
    # トークン検証ロジック
    pass

@router.get("/workspaces")
async def list_workspaces(user: User = Depends(get_current_user)):
    # ユーザーのワークスペースのみ返す
    pass
```

---

### Issue 2: CORS設定が本番環境でセキュリティリスク

**重要度**: 🔴 High

**現状** (`backend/src/app/main.py:23-32`):
```python
app.add_middleware(
    CORSMiddleware,
    allow_origins=(os.environ.get("ALLOW_ORIGIN") or "http://localhost:3000").split(","),
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)
```

**問題点**:
1. `allow_methods=["*"]` でDELETEを含む全HTTPメソッドを許可
2. `allow_headers=["*"]` で任意のヘッダーを許可
3. `allow_credentials=True` と `allow_origins` の組み合わせは要注意
4. デフォルトで `http://localhost:3000` を許可（開発モード判定がない）

**推奨対応**:
```python
# 本番環境では許可するoriginを明示的に制限
allow_methods=["GET", "POST", "PUT", "DELETE", "OPTIONS"]
allow_headers=["Content-Type", "Authorization", "X-Requested-With"]

# 環境変数が設定されていない場合は本番モードではエラー
if not os.environ.get("ALLOW_ORIGIN") and os.environ.get("ENV") == "production":
    raise RuntimeError("ALLOW_ORIGIN must be set in production")
```

---

### Issue 3: Path Traversal の潜在的リスク

**重要度**: 🔴 High

**現状** (`ieapp-cli/src/ieapp/utils.py:10-22`):
```python
def validate_id(identifier: str, name: str) -> None:
    if not identifier or not re.match(r"^[a-zA-Z0-9_-]+$", identifier):
        raise ValueError(...)
```

**良い点**:
- `validate_id` 関数でパストラバーサル攻撃を防止している

**懸念点** (`backend/src/app/api/endpoints/workspaces.py`):
- workspace_id, note_id は validate されているが、APIレベルでは直接パスを操作している
- `ws_path = root_path / "workspaces" / workspace_id` のようにPathオブジェクトを使用（比較的安全）

**推奨対応**:
1. APIレイヤーでも入力バリデーションを追加
2. resolve() を使用してパスを正規化し、ベースパス外へのアクセスを検証

```python
from pathlib import Path

def safe_path(base: Path, *parts: str) -> Path:
    """安全にパスを構築し、ディレクトリトラバーサルを防止"""
    full_path = base.joinpath(*parts).resolve()
    if not str(full_path).startswith(str(base.resolve())):
        raise ValueError("Path traversal detected")
    return full_path
```

---

## 🟠 中優先度 (Medium)

### Issue 4: 例外メッセージによる情報漏洩

**重要度**: 🟠 Medium

**状態**: ✅ 修正済み

**修正前** (`backend/src/app/api/endpoints/workspaces.py`):
```python
except Exception as e:
    logger.exception("Failed to create workspace")
    raise HTTPException(
        status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
        detail=str(e),  # ← 内部エラー情報の漏洩
    ) from e
```

**修正後**:
```python
except Exception:
    logger.exception("Failed to create workspace")
    raise HTTPException(
        status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
        detail="Failed to create workspace",  # 一般的なメッセージ
    ) from None
```

**変更点**:
- すべてのAPIエンドポイントで内部例外メッセージを汎用メッセージに置換
- `from None` で例外チェーンを切断し、内部情報の漏洩を防止
- エラー詳細はサーバーログに記録される

---

### Issue 5: レート制限の欠如

**重要度**: 🟠 Medium

**現状**:
- APIエンドポイントにレート制限がない
- DoS攻撃やブルートフォース攻撃に脆弱

**推奨対応**:
```python
from slowapi import Limiter
from slowapi.util import get_remote_address

limiter = Limiter(key_func=get_remote_address)

@app.get("/")
@limiter.limit("100/minute")
async def root():
    return {"message": "Hello World!"}
```

---

### Issue 6: ログにおける機密情報の潜在的な漏洩

**重要度**: 🟠 Medium

**現状** (`ieapp-cli/src/ieapp/workspace.py:119`):
```python
logger.info("Creating workspace %s at %s", workspace_id, root_path)
```

**懸念点**:
- ファイルパスがログに記録される
- ユーザー情報やセッション情報がログに含まれる可能性

**推奨対応**:
1. 機密情報をログからマスク
2. ログレベルの適切な設定
3. 本番環境でのDEBUGログの無効化

---

### Issue 7: HMAC鍵のローテーション機能不足

**重要度**: 🟠 Medium

**現状** (`backend/src/app/core/security.py`):
- `global.json` に HMAC鍵を保存
- `last_rotation` フィールドがあるが、ローテーション機能が未実装
- LRUキャッシュ (`@lru_cache(maxsize=32)`) により鍵更新後も古い鍵が使用される可能性

**推奨対応**:
1. 鍵ローテーションの定期実行機能
2. キャッシュの適切な無効化
3. 複数鍵のサポート（移行期間用）

---

### Issue 8: X-Forwarded-For ヘッダーの信頼性

**重要度**: 🟠 Medium

**現状** (`backend/src/app/core/security.py:42-48`):
```python
def resolve_client_host(headers, client_host):
    forwarded = headers.get("x-forwarded-for")
    if forwarded:
        candidate = forwarded.split(",", 1)[0].strip()
        if candidate:
            return candidate
    return client_host
```

**問題点**:
- クライアントが任意に X-Forwarded-For ヘッダーを設定可能
- プロキシ経由でない場合でもこのヘッダーが使用される
- IPスプーフィングによるローカルホスト制限のバイパスが可能

**推奨対応**:
1. 信頼できるプロキシからのみヘッダーを受け入れる
2. `TRUSTED_PROXIES` 環境変数の導入

```python
TRUSTED_PROXIES = os.environ.get("TRUSTED_PROXIES", "").split(",")

def resolve_client_host(headers, client_host, request_ip):
    # 信頼されたプロキシからのリクエストのみヘッダーを信頼
    if request_ip in TRUSTED_PROXIES:
        forwarded = headers.get("x-forwarded-for")
        if forwarded:
            return forwarded.split(",", 1)[0].strip()
    return client_host
```

---

## 🟡 低優先度 (Low)

### Issue 9: フロントエンドでのconsole.log出力

**重要度**: 🟡 Low

**状態**: ✅ 修正済み

**修正前** (`frontend/src/lib/api.ts`):
```typescript
console.log(`apiFetch: ${url}`);
```

**問題点**:
- 本番環境でのデバッグ情報の出力
- URL情報がブラウザコンソールに残る

**対応**: console.logを削除済み

---

### Issue 10: サンドボックス実行の監査ログ

**重要度**: 🟡 Low

**現状** (`backend/src/app/mcp/server.py:27`):
```python
logger.info("Executing script for workspace %s", workspace_id)
```

**改善提案**:
- 実行されたコード内容の監査ログ（機密情報をマスク）
- 実行結果のログ
- 失敗した実行の詳細なエラーログ

---

### Issue 11: HTTPOnlyフラグとSecureフラグの設定

**重要度**: 🟡 Low

**現状**:
- Cookieベースの認証が未実装のため現時点で問題なし

**将来的な考慮事項**:
- 認証実装時にはHTTPOnly, Secure, SameSiteフラグを適切に設定

---

### Issue 12: CSPヘッダーの追加

**重要度**: 🟡 Low

**現状**:
- Content-Security-Policy ヘッダーが設定されていない

**推奨対応**:
```python
response.headers["Content-Security-Policy"] = (
    "default-src 'self'; "
    "script-src 'self'; "
    "style-src 'self' 'unsafe-inline'; "
    "img-src 'self' data:; "
)
```

---

## ✅ 良好な実装

### 1. 入力バリデーション
- `validate_id` 関数により、ID文字列が英数字とハイフン、アンダースコアのみに制限
- パストラバーサル攻撃の基本的な防止

### 2. ローカルホストバインディング
- `IEAPP_ALLOW_REMOTE` 環境変数によるリモートアクセス制御
- デフォルトでローカルホストのみ許可

### 3. セキュリティヘッダー
- `X-Content-Type-Options: nosniff` の設定
- HMACによるレスポンス署名

### 4. ファイル権限
- `write_json_secure` 関数で0o600（オーナーのみ読み書き可）に設定
- ディレクトリ作成時に0o700を使用

### 5. OCC (楽観的並行性制御)
- ノート更新時のrevision_idによる競合検出

### 6. サンドボックス実行
- WebAssemblyによるJavaScript実行の分離
- fuel limitによる無限ループ防止

---

## 対応優先度マトリックス

| 優先度 | Issue | 推定対応工数 | 推奨対応時期 |
|--------|-------|-------------|------------|
| 🔴 Critical | 認証・認可の実装 | 大 | 本番運用前必須 |
| 🔴 High | CORS設定の見直し | 小 | 即時 |
| 🔴 High | Path Traversal対策強化 | 小 | 即時 |
| 🟠 Medium | エラーメッセージの見直し | 小 | 1週間以内 |
| 🟠 Medium | レート制限の導入 | 中 | 2週間以内 |
| 🟠 Medium | ログセキュリティ | 小 | 2週間以内 |
| 🟠 Medium | HMAC鍵ローテーション | 中 | 1ヶ月以内 |
| 🟠 Medium | X-Forwarded-For検証 | 小 | 2週間以内 |
| 🟡 Low | console.log削除 | 小 | 随時 |
| 🟡 Low | 監査ログ強化 | 中 | 随時 |
| 🟡 Low | CSPヘッダー追加 | 小 | 随時 |

---

## 参考資料

- [OWASP Top 10](https://owasp.org/www-project-top-ten/)
- [FastAPI Security](https://fastapi.tiangolo.com/tutorial/security/)
- [CORS MDN](https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS)
