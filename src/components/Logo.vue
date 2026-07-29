<template>
  <svg
    viewBox="0 0 64 64"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    class="app-logo"
  >
    <!-- 外框：圆角六边形（跟随主色调） -->
    <path
      class="logo-shell"
      d="M32 4 C36 4 50 11 54 14 C58 17 58 19 58 24 L58 40 C58 45 58 47 54 50 C50 53 36 60 32 60 C28 60 14 53 10 50 C6 47 6 45 6 40 L6 24 C6 19 6 17 10 14 C14 11 28 4 32 4 Z"
    />

    <!-- 内部白色方块 -->
    <path
      d="M32 16 C35 16 44 21 46 23 C48 25 48 26 48 29 L48 35 C48 38 48 39 46 41 C44 43 35 48 32 48 C29 48 20 43 18 41 C16 39 16 38 16 35 L16 29 C16 26 16 25 18 23 C20 21 29 16 32 16 Z"
      fill="#ffffff"
    />

    <!-- 双眼（跟随主色调加深） -->
    <ellipse class="logo-eye" cx="26" cy="32" rx="3" ry="5" />
    <ellipse class="logo-eye" cx="38" cy="32" rx="3" ry="5" />
  </svg>
</template>

<style scoped>
.app-logo {
  display: block;
  width: 100%;
  height: 100%;
}

/* 外框跟随主色调 */
.logo-shell {
  fill: var(--accent, #5b8def);
}

/* 双眼使用主色调的加深色 */
.logo-eye {
  fill: var(--accent-pressed, #3d6bb8);
}
</style>
