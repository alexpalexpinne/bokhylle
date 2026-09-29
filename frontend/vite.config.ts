import { defineConfig, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const pdfWasmDirectory = join(dirname(require.resolve('pdfjs-dist/package.json')), 'wasm')
const pdfWasmFiles = readdirSync(pdfWasmDirectory)

function pdfWasmAssets(): Plugin {
  return {
    name: 'pdf-wasm-assets',
    configureServer(server) {
      server.middlewares.use('/pdfjs/wasm/', (request, response, next) => {
        const filename = request.url?.slice(1)
        if (!filename || !pdfWasmFiles.includes(filename)) return next()
        response.setHeader('Content-Type', filename.endsWith('.wasm') ? 'application/wasm' : 'text/javascript')
        response.end(readFileSync(join(pdfWasmDirectory, filename)))
      })
    },
    generateBundle() {
      for (const filename of pdfWasmFiles) {
        this.emitFile({ type: 'asset', fileName: `pdfjs/wasm/${filename}`, source: readFileSync(join(pdfWasmDirectory, filename)) })
      }
    },
  }
}

export default defineConfig({
  plugins: [react(), tailwindcss(), pdfWasmAssets()],
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:8080',
        changeOrigin: false,
      },
      '/healthz': {
        target: 'http://localhost:8080',
        changeOrigin: false,
      },
    },
  },
})
