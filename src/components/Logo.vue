<template>
  <svg
    viewBox="0 0 64 64"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    class="app-logo"
  >
    <defs>
      <linearGradient id="logoGradient" x1="10" y1="10" x2="54" y2="54" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#4facfe" />
        <stop offset="100%" stop-color="#0061ff" />
      </linearGradient>
    </defs>

    <!-- 外框：圆角六边形 -->
    <path
      d="M32 4 C36 4 50 11 54 14 C58 17 58 19 58 24 L58 40 C58 45 58 47 54 50 C50 53 36 60 32 60 C28 60 14 53 10 50 C6 47 6 45 6 40 L6 24 C6 19 6 17 10 14 C14 11 28 4 32 4 Z"
      fill="url(#logoGradient)"
    />

    <!-- 内部白色方块 -->
    <path
      d="M32 16 C35 16 44 21 46 23 C48 25 48 26 48 29 L48 35 C48 38 48 39 46 41 C44 43 35 48 32 48 C29 48 20 43 18 41 C16 39 16 38 16 35 L16 29 C16 26 16 25 18 23 C20 21 29 16 32 16 Z"
      fill="#ffffff"
    />

    <!-- 双眼 -->
    <ellipse cx="26" cy="32" rx="3" ry="5" fill="#0061ff" />
    <ellipse cx="38" cy="32" rx="3" ry="5" fill="#0061ff" />
  </svg>
</template>

<style scoped>
.app-logo {
  display: block;
  width: 100%;
  height: 100%;
}
</style>
