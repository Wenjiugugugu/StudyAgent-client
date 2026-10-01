// ESLint 扁平配置（flat config）。
//
// 选型说明：
// - 只启用「抓 bug」类规则集：js recommended + typescript-eslint recommended +
//   Vue essential（不引入 Vue 风格类规则，缩进/引号等格式问题交给 Prettier）；
// - `eslint-config-prettier` 必须放在最后，用于关掉与 Prettier 冲突的格式规则；
// - `no-undef` 关闭：TS 的未定义标识符由 `vue-tsc`（pnpm type-check）静态检查，
//   ESLint 侧重复检查还会把 `window`/`document` 等浏览器全局误报成错误。
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import pluginVue from "eslint-plugin-vue";
import prettier from "eslint-config-prettier";

export default [
  {
    // 构建产物、第三方依赖与 Rust 侧不参与前端检查
    ignores: ["dist/**", "dist-old-*/**", "node_modules/**", "src-tauri/**", ".tmp-esbuild/**", "installer/tmp_*/**"],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...pluginVue.configs["flat/essential"],
  {
    files: ["**/*.vue"],
    languageOptions: {
      parserOptions: {
        // <script lang="ts"> 交给 typescript-eslint 解析
        parser: tseslint.parser,
      },
    },
  },
  prettier,
  {
    rules: {
      // 允许以 `_` 开头显式标记「有意未使用」的参数/变量
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrors: "all",
          caughtErrorsIgnorePattern: "^_",
        },
      ],
      // 前端日志是调试面板（aiDebug / useAiRequest 等）的正常手段
      "no-console": "off",
      "no-undef": "off",
      // Markdown/LaTeX 占位符和 URL 安全过滤会刻意匹配控制字符。
      "no-control-regex": "off",
      // 基础 UI 组件使用单词名（Button / Modal / Select 等），是刻意约定
      "vue/multi-word-component-names": "off",
      // 设置分区把同一个响应式表单对象作为编辑上下文传递；允许修改其字段，
      // 但仍禁止替换 prop 本身。该约束与 SettingsPage 的统一保存/取消事务一致。
      "vue/no-mutating-props": ["error", { shallowOnly: true }],
    },
  },
];
