# Win Control Center

一个极致精美、仿 Windows 11 系统风格的任务栏快速控制中心。

## ✨ 特性

- **即时访问**：点击托盘图标，在任务栏上方精确弹出控制面板。
- **极致美学**：完美复刻 Windows 11 “亚克力”透明效果与设计语言。
- **全方位控制**：
  - **屏幕亮度**：支持最低 0% 的极致调光。
  - **系统音量**：主音量与麦克风音量实时控制。
  - **应用混音器**：独立调整每个应用程序的音量。
  - **快速静音**：点击应用图标即可瞬间静音。
  - **鼠标灵敏度**：快速调整系统鼠标移动速度。
- **生产力优化**：
  - **DPI 自适应**：在高 DPI 屏幕上自动保持物理尺寸对齐，绝无模糊与重影。
  - **开机自启**：支持配置开机自动启动。
  - **极其轻量**：基于 Rust (Tauri) 开发，内存占用极低。

## 🚀 快速下载

请前往 [Releases](https://github.com/YGHFv/Win-Control-Center/releases) 页面下载最新的 `.exe` 或 `.msi` 安装包。

## 🛠️ 技术栈

- **后端**: Rust (Tauri v2)
- **前端**: Svelte 5 + Vanilla CSS
- **底层**: Windows COM API (windows-rs), Winreg

## 🏗️ 开发者编译

```bash
# 安装依赖
npm install

# 运行开发预览
npm run tauri dev

# 构建生产包
npm run tauri build
```
