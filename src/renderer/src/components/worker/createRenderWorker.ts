export function createRenderWorker(): Worker {
  return new Worker(new URL('./render/Render.worker.ts', import.meta.url), {
    type: 'module'
  })
}
