vi.mock('../dashboard1/Dashboard1', () => ({
  Dashboard1: 'Dashboard1'
}))

vi.mock('../dashboard2/Dashboard2', () => ({
  Dashboard2: 'Dashboard2'
}))

vi.mock('../dashboard3/Dashboard3', () => ({
  Dashboard3: 'Dashboard3'
}))

vi.mock('../dashboard4/Dashboard4', () => ({
  Dashboard4: 'Dashboard4'
}))

describe('telemetry dashboards index', () => {
  test('re-exports all dashboard modules', async () => {
    const mod = await import('../index')

    expect(mod.Dashboard1).toBe('Dashboard1')
    expect(mod.Dashboard2).toBe('Dashboard2')
    expect(mod.Dashboard3).toBe('Dashboard3')
    expect(mod.Dashboard4).toBe('Dashboard4')
  })
})
