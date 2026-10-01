# StudyAgent 品牌素材

独立书页 Logo 的源文件是 `public/icon.svg`，应用内 Logo 与浏览器图标共用它。
图形以书页和 S 形学习路径为主体，采用柔和蓝色；安装插画使用暖白、鼠尾草绿和纸张元素。

在项目根目录重新生成所有平台图标：

```powershell
node node_modules/@tauri-apps/cli/tauri.js icon public/icon.svg --output src-tauri/icons --ios-color '#F8F6F0'
```

重新生成安装向导插画（需要 Python 和 Pillow，中文字体使用 Windows 微软雅黑）：

```powershell
python installer/assets/generate_assets.py
```

`wizard.bmp` 为 164 × 314 的左侧插画，`wizard-small.bmp` 为 55 × 58 的右上角 Logo。
两者均为 24-bit RGB BMP；小图使用白色底，避免 Inno Setup 6 将透明 BMP 显示为黑底。
预览输出至 `output/branding/studyagent-branding-preview.png`。

修改图标后需要重新编译桌面程序，再生成安装包，才能同时更新应用程序和安装器图标：

```powershell
pnpm build
./installer/build.ps1 -SkipFrontend
```
