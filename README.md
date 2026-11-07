# Model Switcher — Chuyển gateway, API key và model cho Codex, Claude Code, Claude Desktop & Orca (Windows · macOS · Linux)

> Dự án độc lập, không liên kết với OpenAI hay Anthropic.

<!-- Ảnh chụp màn hình: thêm docs/screenshot.png trước khi public -->

## Tính năng

- **Một nơi cho mọi gateway**: thêm nhà cung cấp (URL + key), tải danh sách model, đo độ trễ.
- **Setup từng app một cú bấm**: Codex CLI/Desktop, Claude Code, Claude Desktop (cấu hình bên thứ ba), Orca.
- **Preset gateway**: nhập file `.json` do gateway phát hành hoặc preset mẫu (9router, LiteLLM, OpenRouter), chỉ cần dán key.
- **Kiểm tra khả năng**: gửi request thật để biết gateway có hỗ trợ streaming, tool, prompt caching, ảnh hay không.
- **Doctor**: tìm cấu hình hỏng, biến môi trường cũ gây lỗi 400, proxy, policy quản lý; có nút sửa nhanh.
- **Sử dụng**: thống kê token và tỉ lệ cache từ log trên máy, ước tính chi phí theo giá bạn nhập.
- **Hoàn tác và Khôi phục mặc định**: mỗi thay đổi đều hoàn tác được; một bước đưa các app về như trước khi dùng tool.

## Tải về

> **Lưu ý:** bản Windows đã chạy thử trên máy thật. Bản **macOS và Linux chưa được test** trên máy thật (mới build qua CI); nếu gặp lỗi, hãy mở issue kèm báo cáo hỗ trợ (Cài đặt → Xuất báo cáo).

Bản build **chưa ký số**, nên lần đầu mở hệ điều hành sẽ cảnh báo:

- **Windows** (SmartScreen "Windows protected your PC"): bấm **More info → Run anyway**.
- **macOS** ("không thể mở vì không xác minh được nhà phát triển"): chuột phải vào app → **Open**, hoặc chạy `xattr -dr com.apple.quarantine "/Applications/Model Switcher.app"`.
- **Linux** (AppImage): `chmod +x model-switcher*.AppImage` rồi chạy.

Tool không tự cập nhật; xem bản mới ở trang Releases.

Xem trang [Releases](../../releases). So mã SHA256 của file tải về với mã ghi trên trang Release trước khi chạy:

```powershell
Get-FileHash .\Model.Switcher_x64-setup.exe -Algorithm SHA256
```

```sh
shasum -a 256 Model.Switcher*.dmg   # macOS
sha256sum model-switcher*.AppImage  # Linux
```

## Bắt đầu nhanh

1. **Nhà cung cấp → Thêm** (hoặc **Nhập preset**): điền base URL (thường kết thúc bằng `/v1`) và API key.
2. Bấm **Tải model**.
3. Mở trang app muốn dùng (Codex, Claude Code…), chọn nhà cung cấp và model, bấm **Áp dụng**. Khởi động lại app.

## App hỗ trợ

| App | File được sửa | Ghi chú |
|---|---|---|
| Codex CLI / Desktop | `~/.codex/config.toml` | Tên provider và biến key mặc định là `gateway` / `GATEWAY_API_KEY` (không lộ tên gateway), sửa được ở Nâng cao; catalog cho menu `/model` |
| Claude Code | `~/.claude/settings.json` | Thêm mục "Từ gateway" vào menu `/model` |
| Claude Desktop | `configLibrary` của chế độ bên thứ ba | Không ghi nếu máy có managed policy; xóa được cả cấu hình cũ không do tool tạo (hoàn tác được) |
| Orca | Không cần setup riêng | Orca tự sao `~/.codex/config.toml` sang Codex riêng của nó; tool báo đã đồng bộ chưa, có nút đồng bộ ngay và khởi động lại daemon |

## Bảo mật & hoàn tác

- Key nhà cung cấp nằm trong keychain của hệ điều hành (Windows Credential Manager, macOS Keychain, Secret Service trên Linux), không nằm trong file của tool.
- Một số app bắt buộc đọc key từ file cấu hình của chúng (Claude Code, Claude Desktop). Tool chỉ ghi key vào đúng những chỗ đó.
- Mọi lần ghi đều nguyên tử và có bản sao lưu. Bản sao lưu được mã hóa: DPAPI trên Windows, AES-256-GCM với khóa giữ trong keychain trên macOS/Linux.
- Key không bao giờ xuất hiện trong log, thông báo lỗi, báo cáo hỗ trợ hay file preset.
- **Khôi phục mặc định** (trang Khôi phục) có hai chế độ:
  - *Gỡ phần của tool*: chỉ bỏ những gì tool thêm vào, giữ các chỉnh sửa khác của bạn.
  - *Về bản gốc*: trả file về đúng bản trước lần đầu tool ghi.

## Gỡ lỗi

- Chạy **Doctor** (Cài đặt hoặc Ctrl+K). Mỗi vấn đề có hướng xử lý.
- Gateway trả 400 `reasoning_effort`: xóa biến `CLAUDE_CODE_EFFORT_LEVEL` (Doctor có nút Sửa) và để mức effort mặc định.
- Gửi lỗi: Cài đặt → **Báo cáo hỗ trợ**. Key và thư mục người dùng đã được che, xem lại trước khi gửi.
- Kiểm tra gateway bằng curl:

```sh
curl -s "$BASE_URL/models" -H "Authorization: Bearer $API_KEY"
```

## Build từ source

Cần Node 20+, Rust stable và [phụ thuộc hệ thống của Tauri](https://tauri.app/start/prerequisites/).

```sh
npm ci
npm run tauri dev      # chạy thử
npm run tauri build    # đóng gói
cd src-tauri && cargo test
```

## Giấy phép

[MIT](LICENSE)
