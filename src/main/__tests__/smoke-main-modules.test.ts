import fs from 'node:fs'
import path from 'node:path'

function collectTsFiles(dir: string): string[] {
  const out: string[] = []
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const fullPath = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      out.push(...collectTsFiles(fullPath))
      continue
    }
    if (fullPath.endsWith('.ts') && !fullPath.endsWith('.d.ts') && !fullPath.endsWith('.test.ts')) {
      out.push(fullPath)
    }
  }
  return out
}

describe('main module smoke imports', () => {
  const mainRoot = path.resolve(__dirname, '..')
  const projectRoot = path.resolve(mainRoot, '../..')
  const excluded = new Set([
    path.resolve(mainRoot, 'index.ts'),
    path.resolve(mainRoot, 'services/usb/USBWorker.ts')
  ])

  const files = collectTsFiles(mainRoot)
    .filter((file) => !excluded.has(file))
    .sort((a, b) => a.localeCompare(b))

  test.each(
    files.map((file) => [path.relative(projectRoot, file), file])
  )('imports %s', (_label, filePath) => {
    expect(() => {
      jest.isolateModules(() => {
        require(filePath)
      })
    }).not.toThrow()
  })
})
