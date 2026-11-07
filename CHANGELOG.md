# Changelog

## 0.2.0 (chưa phát hành)

### Thay đổi
- Tiếng Anh là ngôn ngữ nguồn; thêm bản dịch tiếng Việt (`src/i18n/vi.ts`). Đổi ngôn ngữ ở thanh bên hoặc Cài đặt.
- Codex: tên provider và biến môi trường mặc định là `gateway` / `GATEWAY_API_KEY` thay cho `ms-<id>` / `MS_<ID>_API_KEY`; sửa được ở Nâng cao. Setup lại sẽ thay bảng tên cũ và gỡ biến cũ nếu giữ cùng key (hoàn tác được).
- Header tùy chỉnh theo nhà cung cấp: ghi vào `ANTHROPIC_CUSTOM_HEADERS` (Claude Code) và `inferenceCustomHeaders` (Claude Desktop). Trang Claude có mục gỡ lỗi 400 `reasoning_effort` với nút thêm header tương thích. Preset mang theo header (không chứa key).
- Kiểm tra khả năng gateway: tích model ngay trong bảng model, kết quả hiện cạnh tên model.
- Orca: bỏ form setup riêng. Orca tự mirror `~/.codex/config.toml`; trang Orca chỉ báo trạng thái, đồng bộ ngay và khởi động lại daemon.
- Khi chạy trong terminal của Orca, tool bỏ qua `CODEX_HOME` do Orca gán và dùng `~/.codex`.
- Claude Desktop: chọn và xóa cấu hình bất kỳ trong configLibrary, kể cả cấu hình tạo trong app hoặc bởi tool cũ.
- Windows: thanh tiêu đề tích hợp thay khung cửa sổ mặc định. Thao tác phụ gom vào menu ⋯, giải thích dài chuyển vào ⓘ.
- Codex: hướng dẫn tạo ảnh qua gateway (`/images/generations`, mặc định PNG nền trong suốt) ghi vào `AGENTS.md`, không chứa key, hoàn tác được.
- Header sửa lỗi 400 `reasoning_effort` đổi thành giá trị trung tính `User-Agent: model-switcher/1` + `x-app: model-switcher`.
- Restart Claude Desktop: chỉ tắt đúng Claude Desktop (không đụng Claude Code CLI), chờ tắt hẳn, mở lại và kiểm tra app đã chạy.
- Restart Claude Desktop và Codex Desktop trên macOS (`osascript quit` → SIGTERM → SIGKILL, mở lại bằng `open -a`) và Claude Desktop trên Linux. Chưa chạy thử trên máy thật.
- Ghi `restart.log` từng bước restart; báo cáo hỗ trợ kèm 60 dòng cuối.
- Trang Sử dụng lưu kết quả từng file log (theo hash đường dẫn, kích thước, thời gian sửa), mở lại chỉ đọc file mới.
- Bảng model: cột độ trễ riêng, nhãn hết quota theo nhóm, nút Thử hiện khi rê chuột.
- Orca trên macOS/Linux: tìm Codex riêng của Orca theo đúng cách Orca chọn thư mục dữ liệu (`ORCA_USER_DATA_PATH`, `~/Library/Application Support/orca`, `$XDG_CONFIG_HOME/orca` hoặc `~/.config/orca`). Khởi động lại daemon Orca vẫn chỉ hỗ trợ Windows.
- Bản macOS và Linux chưa test trên máy thật. Bản build chưa ký số và chưa tự cập nhật.

## 0.1.0 (chưa phát hành)

Bản viết lại bằng Tauri 2 của Model Switcher.

### Thêm
- Nhà cung cấp: lưu key trong keychain của hệ điều hành, tải danh sách model, đo độ trễ.
- File preset gateway (`model-switcher/preset@1`): nhập từ file hoặc preset mẫu, xuất preset (không bao giờ chứa key).
- Kiểm tra khả năng gateway theo từng model: endpoint, streaming, tool, prompt caching, ảnh.
- Setup Codex (CLI, Desktop), Claude Code, Claude Desktop, Orca. Khởi động lại Codex Desktop và daemon Orca.
- Mọi thay đổi ghi file đều nguyên tử, có bản sao lưu mã hóa (DPAPI trên Windows, AES-GCM với khóa trong keychain trên macOS/Linux) và hoàn tác được.
- Khôi phục mặc định: một bước gỡ mọi thứ tool đã thêm, hoặc trả file về bản gốc trước lần đầu tool ghi. Bản thân thao tác này cũng hoàn tác được.
- Doctor với nút sửa nhanh; báo cáo hỗ trợ đã che key.
- Trang Sử dụng đọc log trên máy: token, tỉ lệ cache, ước tính chi phí theo giá người dùng nhập.
- Bảng lệnh Ctrl+K, giao diện sáng/tối.
