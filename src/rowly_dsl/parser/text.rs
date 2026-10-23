//! 文字列の分割・大文字小文字無視比較・終端判定を行う。

use super::*;

// {
//   責務: [strip_keyword: 大文字小文字を無視して先頭キーワードを取り除く]
//   引数: [text: 入力文字列, keyword: キーワード]
//   戻り値: [Option<&str>: 単語境界が一致した場合の残り文字列]
// }
pub(super) fn strip_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = strip_prefix_ci(text, keyword)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim_start())
    } else {
        None
    }
}

// {
//   責務: [split_member: 最上位のドットで対象名とメンバー名を分割する]
//   引数: [text: メンバー参照文字列]
//   戻り値: [Option<(&str, &str)>: 対象とメンバー]
// }
pub(super) fn split_member(text: &str) -> Option<(&str, &str)> {
    split_top_level_token(text, ".")
}

// {
//   責務: [split_top_level_once: 最上位にある指定文字の最初の位置で分割する]
//   引数: [text: 入力文字列, target: 分割文字]
//   戻り値: [Option<(&str, &str)>: 左側と右側]
// }
pub(super) fn split_top_level_once(text: &str, target: char) -> Option<(&str, &str)> {
    let token = target.to_string();
    split_top_level_token(text, &token)
}

// {
//   責務: [split_top_level_token: 括弧・引用符の外の最初のトークンで分割する]
//   引数: [text: 入力文字列, token: 分割トークン]
//   戻り値: [Option<(&str, &str)>: トークン前後の部分文字列]
// }
pub(super) fn split_top_level_token<'a>(text: &'a str, token: &str) -> Option<(&'a str, &'a str)> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index + token.len() <= text.len() {
        let ch = text[index..].chars().next()?;
        if escaped {
            escaped = false;
            index += ch.len_utf8();
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            index += 1;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            index += 1;
            continue;
        }
        if !quoted {
            match ch {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                _ => {}
            }
            if depth == 0 && bytes[index..].starts_with(token.as_bytes()) {
                return Some((&text[..index], &text[index + token.len()..]));
            }
        }
        index += ch.len_utf8();
    }
    None
}

// {
//   責務: [split_keyword_top_level: 大文字小文字を無視し最上位キーワードで分割する]
//   引数: [text: 入力文字列, keyword: 分割キーワード]
//   戻り値: [Option<(&str, &str)>: キーワード前後の部分文字列]
// }
pub(super) fn split_keyword_top_level<'a>(
    text: &'a str,
    keyword: &str,
) -> Option<(&'a str, &'a str)> {
    let lower = text.to_ascii_lowercase();
    let keyword_lower = keyword.to_ascii_lowercase();
    let mut search_from = 0usize;
    while let Some(relative) = lower[search_from..].find(&keyword_lower) {
        let index = search_from + relative;
        if is_top_level_index(text, index) {
            return Some((&text[..index], &text[index + keyword.len()..]));
        }
        search_from = index + 1;
    }
    None
}

// {
//   責務: [is_top_level_index: 指定バイト位置が括弧・引用符の外か判定する]
//   引数: [text: 入力文字列, target: 判定位置]
//   戻り値: [bool: 最上位かつ引用符外ならtrue]
// }
pub(super) fn is_top_level_index(text: &str, target: usize) -> bool {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if index >= target {
            break;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth == 0 && !quoted
}

// {
//   責務: [strip_outer_parentheses: 式全体を囲う対応済み括弧を繰り返し取り除く]
//   引数: [text: 式文字列]
//   戻り値: [&str: 外側括弧を除いたトリム済み式]
// }
pub(super) fn strip_outer_parentheses(mut text: &str) -> &str {
    loop {
        let trimmed = text.trim();
        if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
            return trimmed;
        }
        if find_closing_parenthesis(trimmed, 1) != Some(trimmed.len() - 1) {
            return trimmed;
        }
        text = &trimmed[1..trimmed.len() - 1];
    }
}

// {
//   責務: [validate_identifier: 識別子形式を検証し不正時に行番号付きエラーを返す]
//   引数: [identifier: 識別子, line: 元の1始まり行番号]
//   戻り値: [Result<(), ParseError>: 有効ならOk(())]
//   エラー: [ParseError: 識別子形式が不正]
// }
pub(super) fn validate_identifier(identifier: &str, line: usize) -> Result<(), ParseError> {
    if is_identifier(identifier) {
        Ok(())
    } else {
        Err(parse_error(
            line,
            format!("invalid identifier `{identifier}`"),
        ))
    }
}

// {
//   責務: [is_identifier: 先頭が英字または下線の識別子形式を判定する]
//   引数: [identifier: 判定対象文字列]
//   戻り値: [bool: 以降も英数字または下線ならtrue]
// }
pub(super) fn is_identifier(identifier: &str) -> bool {
    let mut chars = identifier.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic()) && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

// {
//   責務: [is_terminator: End If、End Def、End Classのいずれかを判定する]
//   引数: [text: ソース行]
//   戻り値: [bool: ブロック終端ならtrue]
// }
pub(super) fn is_terminator(text: &str) -> bool {
    is_end_if(text) || is_end_def(text) || is_end_class(text)
}

// {
//   責務: [is_end_if: End IfまたはEndIf表記を判定する]
//   引数: [text: ソース行]
//   戻り値: [bool: Ifブロック終端ならtrue]
// }
pub(super) fn is_end_if(text: &str) -> bool {
    eq_ci(text, "end if") || eq_ci(text, "endif")
}

// {
//   責務: [is_end_def: End DefまたはEndDef表記を判定する]
//   引数: [text: ソース行]
//   戻り値: [bool: 関数ブロック終端ならtrue]
// }
pub(super) fn is_end_def(text: &str) -> bool {
    eq_ci(text, "end def") || eq_ci(text, "enddef")
}

// {
//   責務: [is_end_class: End ClassまたはEndClass表記を判定する]
//   引数: [text: ソース行]
//   戻り値: [bool: クラスブロック終端ならtrue]
// }
pub(super) fn is_end_class(text: &str) -> bool {
    eq_ci(text, "end class") || eq_ci(text, "endclass")
}

// {
//   責務: [is_rem_comment: REM単独またはREMで始まる行を判定する]
//   引数: [text: ソース行]
//   戻り値: [bool: REMコメント行ならtrue]
// }
pub(super) fn is_rem_comment(text: &str) -> bool {
    eq_ci(text, "rem") || starts_with_ci(text, "rem ")
}

// {
//   責務: [starts_with_ci: 大文字小文字を無視して接頭辞一致を判定する]
//   引数: [text: 入力文字列, prefix: 接頭辞]
//   戻り値: [bool: 接頭辞が一致すればtrue]
// }
pub(super) fn starts_with_ci(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
}

// {
//   責務: [strip_prefix_ci: 大文字小文字を無視して一致した接頭辞を除去する]
//   引数: [text: 入力文字列, prefix: 接頭辞]
//   戻り値: [Option<&str>: 一致した場合の残り文字列]
// }
pub(super) fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    starts_with_ci(text, prefix).then(|| &text[prefix.len()..])
}

// {
//   責務: [eq_ci: 2つの文字列を大文字小文字を無視して比較する]
//   引数: [left: 左辺文字列, right: 右辺文字列]
//   戻り値: [bool: ASCII大小文字を無視して等しければtrue]
// }
pub(super) fn eq_ci(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

// {
//   責務: [split_once_ci: 最初の区切り文字列で大小文字を無視して分割する]
//   引数: [text: 入力文字列, separator: 区切り文字列]
//   戻り値: [Option<(&str, &str)>: 区切り前後の部分文字列]
// }
pub(super) fn split_once_ci<'a>(text: &'a str, separator: &str) -> Option<(&'a str, &'a str)> {
    let lower = text.to_ascii_lowercase();
    let index = lower.find(&separator.to_ascii_lowercase())?;
    Some((&text[..index], &text[index + separator.len()..]))
}

// {
//   責務: [parse_error: 行番号とメッセージから解析エラーを作成する]
//   引数: [line: 元の1始まり行番号, message: エラーメッセージ]
//   戻り値: [ParseError: 行番号と本文を保持したエラー]
// }
pub(super) fn parse_error(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}
