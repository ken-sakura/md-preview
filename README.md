# md-preview

`md-preview` は、ターミナル（CUI）でMarkdownファイルをプレビューするためのRust製ツールです。

ファイルエクスプローラーでMarkdownファイルを選択し、その場でレンダリングされた内容を確認できます。

## 特徴

-   **ファイルエクスプローラー**: ディレクトリを移動し、ファイルを選択できます。
-   **Markdownプレビュー**: 選択したMarkdownファイルをターミナル上で美しくレンダリングします。
-   **Vimライクなキー操作**: `j`, `k`, `h`, `l` などのキーで直感的に操作できます。
-   **シンタックスハイライト**: コードブロックを色付きで表示します。
-   **テーブル表示**: Markdownのテーブルを罫線付きで表示します。

## スクリーンショット

### エクスプローラー画面
![エクスプローラー画面の画像](https://placehold.co/600x400/2d3748/ffffff?text=Explorer+View)
*ディレクトリとファイルの一覧が表示されます。*

### プレビュー画面
![プレビュー画面の画像](https://placehold.co/600x400/1a202c/ffffff?text=Preview+View)
*選択したMarkdownファイルがレンダリングされます。*


## インストール

1.  [Rustのツールチェーン](https://www.rust-lang.org/tools/install)をインストールします。
2.  このリポジトリをクローンします。
    ```bash
    git clone <repository_url>
    cd md-preview
    ```
3.  `cargo` を使ってビルド・インストールします。
    ```bash
    cargo install --path .
    ```

## 使い方

ターミナルで以下のコマンドを実行すると、カレントディレクトリでエクスプローラーが起動します。

```bash
md-preview
