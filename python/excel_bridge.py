#!/usr/bin/env python3
import json
import sys

from openpyxl import Workbook, load_workbook


# {
#   責務: [read_request: stdinからUTF-8 JSON requestを読み取りPython値へ変換する。]
#   戻り値: [object: JSON protocolで受信したrequest。]
#   エラー: [不正UTF-8または不正JSONの場合はdecode・JSON parse errorを送出する。]
# }
def read_request():
    # PyInstaller executable の標準入力 encoding は Windows の code page に
    # 依存し得るため、JSON protocol は text stream を経由せず UTF-8 bytes で固定する。
    return json.loads(sys.stdin.buffer.read().decode("utf-8"))


# {
#   責務: [write_response: payloadをUTF-8 JSONとしてstdoutへ返す。]
#   引数: [payload: bridge responseとしてserializeする値。]
#   戻り値: [None: 値を返さない。]
#   副作用: [stdoutへresponse bytesを書き込みflushする。]
# }
def write_response(payload):
    data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    sys.stdout.buffer.write(data)
    sys.stdout.buffer.flush()


# {
#   責務: [stringify: worksheet cell valueをCSVで保持する文字列形式へ変換する。]
#   引数: [value: openpyxlが返したworksheet cell value。]
#   戻り値: [str: Noneは空文字、boolは小文字表記、それ以外はPython文字列表現。]
# }
def stringify(value):
    if value is None:
        return ""
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


# {
#   責務: [export_workbook: JSON requestのrowsを文字列cellとしてExcel workbookへ保存する。]
#   引数: [request: output_path・sheet_name・rowsを含むexport payload。]
#   戻り値: [None: 値を返さない。]
#   副作用: [指定pathへworkbookを保存し、stdoutへ成功responseを書く。]
# }
def export_workbook(request):
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = request["sheet_name"]

    for row_index, row in enumerate(request["rows"], start=1):
        for column_index, value in enumerate(row, start=1):
            cell = sheet.cell(row=row_index, column=column_index)
            cell.value = value
            cell.data_type = "s"
            cell.number_format = "@"

    workbook.save(request["output_path"])
    write_response({"ok": True})


# {
#   責務: [import_workbook: 指定worksheetまたはactive sheetをCSV用の文字列rowsとして返す。]
#   引数: [request: input_pathと任意のsheet_nameを含むimport payload。]
#   戻り値: [None: rowsをJSON responseとしてstdoutへ書く。]
#   副作用: [workbookをread-onlyで開き、stdoutへresponseを書く。]
#   エラー: [指定sheetが存在しない場合は利用可能なsheet名を含むValueErrorを送出する。]
# }
def import_workbook(request):
    workbook = load_workbook(request["input_path"], data_only=False, read_only=True)
    sheet_name = request.get("sheet_name")
    if sheet_name is None:
        sheet = workbook.active
    else:
        if sheet_name not in workbook.sheetnames:
            raise ValueError(
                f"worksheet not found: {sheet_name!r}; available: {workbook.sheetnames!r}"
            )
        sheet = workbook[sheet_name]

    rows = []
    for raw_row in sheet.iter_rows(values_only=True):
        values = list(raw_row)
        while values and values[-1] is None:
            values.pop()
        rows.append([stringify(value) for value in values])

    while rows and not rows[-1]:
        rows.pop()

    write_response({"rows": rows})


# {
#   責務: [main: CLI modeを検査し、標準入力requestに対応するbridge処理を実行する。]
#   戻り値: [None: 値を返さない。]
#   エラー: [未知のmodeまたは引数形式の場合はusageを示すSystemExitを送出する。]
# }
def main():
    if len(sys.argv) != 2 or sys.argv[1] not in {"export", "import"}:
        raise SystemExit("usage: excel_bridge.py <export|import>")

    request = read_request()
    if sys.argv[1] == "export":
        export_workbook(request)
    else:
        import_workbook(request)


if __name__ == "__main__":
    main()
