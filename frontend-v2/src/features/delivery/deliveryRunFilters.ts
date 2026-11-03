type RunWithConfigurationId = {
  configurationId?: unknown
  configuration_id?: unknown
  ruleId?: unknown
  rule_id?: unknown
}

export function filterRunsForConfiguration<T extends RunWithConfigurationId>(runs: T[], configurationId: unknown) {
  const targetId = Number(configurationId)
  if (!Number.isFinite(targetId) || targetId <= 0) return []
  return runs.filter((run) => {
    const runConfigurationId = Number(run.configurationId ?? run.configuration_id ?? run.ruleId ?? run.rule_id)
    return Number.isFinite(runConfigurationId) && runConfigurationId === targetId
  })
}
