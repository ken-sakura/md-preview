import os
import re

# --- 設定 ---
# プロジェクトのルートからの相対パスを定義
DECK_MD_PATH = 'docs/DECK.md'
CARDS_DIR = 'docs/cards'
TEMPLATE_PATH = 'templates/SCREEN_CARD_TEMPLATE.md'
# --- 設定ここまで ---

def parse_deck_file(filepath):
    """DECK.mdファイルを解析し、カードIDとカード名のリストを返す"""
    card_list = []
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            content = f.read()
    except FileNotFoundError:
        print(f"エラー: {filepath} が見つかりません。")
        return []

    # カード一覧のセクションから情報を抽出する正規表現
    # 例: * `CARD-001`: ログイン画面
    card_regex = re.compile(r"^\*\s*`([A-Z]+-\d+)`:\s*(.*)", re.MULTILINE)

    for match in card_regex.finditer(content):
        card_id = match.group(1)
        card_name = match.group(2).strip()
        card_list.append({'id': card_id, 'name': card_name})
        
    return card_list

def create_card_files(card_info, template_content):
    """指定されたカード情報に基づいてフォルダとファイルを作成する"""
    card_id = card_info['id']
    card_name = card_info['name']
    
    # フォルダ名はカードIDのみにする
    folder_path = os.path.join(CARDS_DIR, card_id)
    file_path = os.path.join(folder_path, 'index.md')

    # フォルダが存在しない場合は作成
    if not os.path.exists(folder_path):
        os.makedirs(folder_path)
        print(f"フォルダを作成しました: {folder_path}")
    else:
        print(f"フォルダは既に存在します: {folder_path}")

    # index.mdがまだ存在しない場合のみ作成
    if not os.path.exists(file_path):
        # テンプレートのプレースホルダーを置換
        content = template_content.replace('(例: CARD-000)', card_id)
        content = content.replace('(画面名)', card_name)
        
        with open(file_path, 'w', encoding='utf-8') as f:
            f.write(content)
        print(f"  -> ファイルを作成しました: {file_path}")
    else:
        print(f"  -> ファイルは既に存在します: {file_path}")

def main():
    """メイン処理"""
    print("DECK.mdからカードの雛形生成を開始します...")
    
    # テンプレートファイルを読み込む
    try:
        with open(TEMPLATE_PATH, 'r', encoding='utf-8') as f:
            template = f.read()
    except FileNotFoundError:
        print(f"エラー: テンプレートファイル {TEMPLATE_PATH} が見つかりません。")
        return

    # DECK.mdからカード一覧を取得
    cards = parse_deck_file(DECK_MD_PATH)
    
    if not cards:
        print("DECK.mdからカードが見つかりませんでした。")
        return
        
    print(f"{len(cards)}件のカードが見つかりました。")

    # 各カードのファイルとフォルダを作成
    for card in cards:
        create_card_files(card, template)
        
    print("\n処理が完了しました。")

if __name__ == "__main__":
    main()

