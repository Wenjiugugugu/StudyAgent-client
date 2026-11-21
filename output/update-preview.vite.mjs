import { mergeConfig } from 'vite';
import base from '../vite.config.ts';
export default mergeConfig(base, { server: {
  host: '127.0.0.1', port: 15329, strictPort: true,
  watch: { ignored: ['**/src-tauri/**'] },
} });
