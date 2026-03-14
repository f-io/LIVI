import type { ProjectionWorker } from './types'

export function createProjectionWorker(): ProjectionWorker {
  return new Worker(new URL('./Projection.worker.ts', import.meta.url), {
    type: 'module'
  }) as ProjectionWorker
}
